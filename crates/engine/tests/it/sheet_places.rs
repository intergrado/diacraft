//! **A row command keeps the place of every unknown node** of the section, the sheet and the shape it changes.
//!
//! The model keeps an unknown node (a foreign element, a comment) as the count of typed children that stood before it (`Placed::after`), and a model writer puts it back
//! after that many. A command that puts a row, a section or a cell before the node, or takes one out, has to move the count with it, or the node is written one place off:
//! a comment that stood after row 2 stands after row 1 (or row 3) in the file a model writer makes (a `.diacraft` file saved as a VSDX file, a Save As across the formats).
//!
//! The drawing is [`PACKAGE`]: a shape whose `Scratch` section has an unknown element before its first row, one after its second and one after its last, a `User` section of named
//! rows with one between them, a comment between the two sections, an unknown element after the last section, and an instance of a master whose section holds the same kind of nodes.
//! **Every row of `Scratch` carries an unknown attribute of its own** (`K`), so that each row is told apart by what it holds and the splice into a VSDX file, which matches a row by
//! that, follows the same row the model does: **the outline of the file a model writer makes is the outline of the file a splice makes**, and both are what [`Case::expect`] says.
//!
//! An outline lists the children of the shape that are not cells, in order: a section as `Name[its rows and its unknown nodes]`, a row by its `K` or its name (`new` for a row
//! the command made), an unknown element as `?Name` and a comment as `#text`.

use diacraft_container::xml::{El, Node};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::sheet_files_world::parts_of;

const X: &str = r#"xmlns:x="urn:test:unknown""#;

fn shapes() -> String {
    format!(
        r##"
<Shape ID="1" Type="Shape" NameU="Box" Name="Box">
  <Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
  <Section N="Scratch">
    <x:Lead {X}/>
    <Row IX="0" K="r1"><Cell N="A" V="10"/></Row>
    <Row IX="1" K="r2"><Cell N="A" V="20"/></Row>
    <x:Hint {X}/>
    <Row IX="2" K="r3"><Cell N="A" V="30"/></Row>
    <x:Trail {X}/>
  </Section>
  <!-- between the sections -->
  <Section N="User">
    <Row N="Rate"><Cell N="Value" V="5"/></Row>
    <x:Mid {X}/>
    <Row N="Total"><Cell N="Value" V="10"/></Row>
  </Section>
  <x:Foreign {X}/>
</Shape>
<Shape ID="2" Type="Shape" Master="1" NameU="Tmpl.2" Name="Tmpl.2">
  <Cell N="PinX" V="4"/><Cell N="PinY" V="4"/>
  <Section N="Scratch">
    <x:Lead2 {X}/>
    <Row IX="1" K="i1"><Cell N="A" V="22"/></Row>
    <x:Note {X}/>
  </Section>
  <x:Tail {X}/>
</Shape>"##
    )
}

const MASTER_SHAPE: &str = r#"
<Shape ID="5" Type="Shape" NameU="Tmpl" Name="Tmpl">
  <Cell N="Width" V="1"/><Cell N="Height" V="1"/>
  <Section N="Scratch">
    <Row IX="0"><Cell N="A" V="1"/></Row>
    <Row IX="1"><Cell N="A" V="2"/></Row>
    <Row IX="2"><Cell N="A" V="3"/></Row>
  </Section>
</Shape>"#;

/// The drawing as a VSDX file.
fn package() -> Vec<u8> {
    VsdxBuilder::new().master(MasterSpec::new(1, "Tmpl").shapes(MASTER_SHAPE)).page(PageSpec::new(1, "Page-1").shapes(&shapes())).build()
}

/// The tail of the name of the first page's part: the part is found by it among the parts of the package, whatever folder the package keeps its parts in.
const PAGE_PART_TAIL: &str = "/pages/page1.xml";

// ------------------------------------------------------------------------------------------------------------------------------ outlines

/// What stands for a row in an outline: its `K`, else its name, else `new`.
fn label(row: &El) -> String {
    row.attr("K").or_else(|| row.attr("N")).unwrap_or("new").to_owned()
}

/// A section as `Name[...]`: its rows and the nodes between them.
fn section_outline(section: &El) -> String {
    let mut inner: Vec<String> = Vec::new();
    for node in &section.children {
        match node {
            Node::El(child) if child.is("Row") => inner.push(label(child)),
            Node::El(child) if child.is("Cell") => {}
            Node::El(child) => inner.push(format!("?{}", child.local())),
            Node::Comment(text) => inner.push(format!("#{}", text.trim())),
            Node::Text(_) | Node::CData(_) | Node::Pi { .. } => {}
        }
    }
    format!("{}[{}]", section.attr("N").unwrap_or(""), inner.join(","))
}

/// The children of the shape `id` that are not cells, in order.
fn outline(package: &[u8], id: &str) -> Vec<String> {
    let parts = parts_of(package);
    let part = parts.iter().find(|(name, _)| name.ends_with(PAGE_PART_TAIL)).map(|(_, bytes)| bytes).expect("the package has no first page");
    let root = diacraft_opc::xml::parse(part).unwrap();
    let shape = root
        .elements()
        .filter(|e| e.is("Shapes"))
        .flat_map(|shapes| shapes.elements())
        .find(|s| s.is("Shape") && s.attr("ID") == Some(id))
        .unwrap_or_else(|| panic!("no shape {id} in the page"));
    let mut out = Vec::new();
    for node in &shape.children {
        match node {
            Node::El(child) if child.is("Section") => out.push(section_outline(child)),
            Node::El(child) if child.is("Cell") => {}
            Node::El(child) => out.push(format!("?{}", child.local())),
            Node::Comment(text) => out.push(format!("#{}", text.trim())),
            Node::Text(_) | Node::CData(_) | Node::Pi { .. } => {}
        }
    }
    out
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

// -------------------------------------------------------------------------------------------------------------------------------- cases

const SHAPE_1: &str = r#"{"id":1}"#;
const SHAPE_2: &str = r#"{"id":2}"#;

fn call(s: &mut Session, command: &str, target: &str, extra: Value) {
    let mut params: Value = serde_json::from_str(target).unwrap();
    if let (Some(map), Some(more)) = (params.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
}

struct Case {
    name: &'static str,
    /// The commands, and how many there are (each is one undo step).
    run: fn(&mut Session),
    steps: usize,
    /// The shape that is read back, and its outline after the commands.
    shape: &'static str,
    expect: Vec<String>,
}

/// What the shape `1` is before any command.
fn shape_1() -> Vec<String> {
    words(&["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", "#between the sections", "User[Rate,?Mid,Total]", "?Foreign"])
}

/// What the shape `2` (the instance) is before any command.
fn shape_2() -> Vec<String> {
    words(&["Scratch[?Lead2,i1,?Note]", "?Tail"])
}

fn cases() -> Vec<Case> {
    let mut list = Vec::new();
    let mut case = |name: &'static str, steps: usize, run: fn(&mut Session), shape: &'static str, expect: &[&str]| {
        list.push(Case { name, run, steps, shape, expect: words(expect) });
    };
    let (comment, user, foreign) = ("#between the sections", "User[Rate,?Mid,Total]", "?Foreign");
    // A row before the first: the node before the first row stays before it, and the node that stood after row 2 still stands after it.
    case(
        "an indexed row before row 1",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch", "before": 1})),
        "1",
        &["Scratch[?Lead,new,r1,r2,?Hint,r3,?Trail]", comment, user, foreign],
    );
    // A row after row 2 and a row before row 3 are the same place: the new row goes ahead of the node that stood there.
    case(
        "an indexed row after row 2",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch", "after": 2})),
        "1",
        &["Scratch[?Lead,r1,r2,new,?Hint,r3,?Trail]", comment, user, foreign],
    );
    case(
        "an indexed row before row 3",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch", "before": 3})),
        "1",
        &["Scratch[?Lead,r1,r2,new,?Hint,r3,?Trail]", comment, user, foreign],
    );
    case(
        "an indexed row after row 1",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch", "after": 1})),
        "1",
        &["Scratch[?Lead,r1,new,r2,?Hint,r3,?Trail]", comment, user, foreign],
    );
    // A row at the end goes ahead of the node that trails the last row.
    case(
        "an indexed row at the end",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,new,?Trail]", comment, user, foreign],
    );
    // Deletes: the node that stood after the row goes with the row before it.
    case(
        "delete the row just before the node",
        1,
        |s| call(s, "sheet.delete_row", SHAPE_1, json!({"section": "Scratch", "row": 2})),
        "1",
        &["Scratch[?Lead,r1,?Hint,r3,?Trail]", comment, user, foreign],
    );
    case(
        "delete the first row",
        1,
        |s| call(s, "sheet.delete_row", SHAPE_1, json!({"section": "Scratch", "row": 1})),
        "1",
        &["Scratch[?Lead,r2,?Hint,r3,?Trail]", comment, user, foreign],
    );
    case(
        "delete the row just after the node",
        1,
        |s| call(s, "sheet.delete_row", SHAPE_1, json!({"section": "Scratch", "row": 3})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,?Trail]", comment, user, foreign],
    );
    // Named rows.
    case(
        "a named row after Rate",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "User", "name": "X", "after": "Rate"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, "User[Rate,X,?Mid,Total]", foreign],
    );
    case(
        "a named row before Total",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "User", "name": "X", "before": "Total"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, "User[Rate,X,?Mid,Total]", foreign],
    );
    case(
        "a named row at the end",
        1,
        |s| call(s, "sheet.add_row", SHAPE_1, json!({"section": "User", "name": "X"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, "User[Rate,?Mid,Total,X]", foreign],
    );
    case(
        "delete the named row before the node",
        1,
        |s| call(s, "sheet.delete_row", SHAPE_1, json!({"section": "User", "row": "Rate"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, "User[?Mid,Total]", foreign],
    );
    case(
        "delete the named row after the node",
        1,
        |s| call(s, "sheet.delete_row", SHAPE_1, json!({"section": "User", "row": "Total"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, "User[Rate,?Mid]", foreign],
    );
    // Several commands one after the other.
    case(
        "a row before row 1, then the row that is now the third",
        2,
        |s| {
            call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch", "before": 1}));
            call(s, "sheet.delete_row", SHAPE_1, json!({"section": "Scratch", "row": 3}));
        },
        "1",
        &["Scratch[?Lead,new,r1,?Hint,r3,?Trail]", comment, user, foreign],
    );
    case(
        "a row at the end, then the first, then one in the middle",
        3,
        |s| {
            call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch"}));
            call(s, "sheet.delete_row", SHAPE_1, json!({"section": "Scratch", "row": 1}));
            call(s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch", "after": 1}));
        },
        "1",
        &["Scratch[?Lead,r2,new,?Hint,r3,new,?Trail]", comment, user, foreign],
    );
    // Sections: the nodes among the shape's sections.
    case(
        "a section added at the end goes ahead of the node that trails the sections",
        1,
        |s| call(s, "sheet.add_section", SHAPE_1, json!({"section": "Hyperlink"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, user, "Hyperlink[Row_1]", foreign],
    );
    case(
        "the section that follows the comment is deleted",
        1,
        |s| call(s, "sheet.delete_section", SHAPE_1, json!({"section": "User"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, foreign],
    );
    case(
        "the first section is deleted: the comment that followed it leads",
        1,
        |s| call(s, "sheet.delete_section", SHAPE_1, json!({"section": "Scratch"})),
        "1",
        &[comment, user, foreign],
    );
    // Cells that a section command makes directly under the sheet go among the cells, which stand before every section.
    case(
        "the Text Transform cells",
        1,
        |s| call(s, "sheet.add_section", SHAPE_1, json!({"section": "Text Transform"})),
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, user, foreign],
    );
    case(
        "the Text Transform cells added and deleted",
        2,
        |s| {
            call(s, "sheet.add_section", SHAPE_1, json!({"section": "Text Transform"}));
            call(s, "sheet.delete_section", SHAPE_1, json!({"section": "Text Transform"}));
        },
        "1",
        &["Scratch[?Lead,r1,r2,?Hint,r3,?Trail]", comment, user, foreign],
    );
    // An instance: the section its master gives is copied into it before an insert in the middle, and the section it had the master's rows are put around.
    case(
        "an instance: a row after the first of what the master gives (copy, then insert)",
        1,
        |s| call(s, "sheet.add_row", SHAPE_2, json!({"section": "Scratch", "after": 1})),
        "2",
        &["Scratch[?Lead2,new,new,i1,new,?Note]", "?Tail"],
    );
    case(
        "an instance: a row at the end (no copy)",
        1,
        |s| call(s, "sheet.add_row", SHAPE_2, json!({"section": "Scratch"})),
        "2",
        &["Scratch[?Lead2,i1,new,?Note]", "?Tail"],
    );
    list
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
    let seen = outline(got, case.shape);
    if seen != expect {
        failures.push(format!("[{what}] {}:\n      want {expect:?}\n      got  {seen:?}", case.name));
    }
}

fn report(failures: Vec<String>, total: usize) {
    assert!(failures.is_empty(), "{} of {total} trips failed:\n{}", failures.len(), failures.join("\n"));
}

/// The drawing is what the outlines above say before any command, in the file and in the one a model writer makes of it.
#[test]
fn the_drawing_stands_as_its_outline_says_in_the_file_and_in_what_a_model_writer_makes_of_it() {
    let dir = Dir::new();
    let vsdx = dir.write("a.vsdx", &package());
    assert_eq!(outline(&package(), "1"), shape_1());
    assert_eq!(outline(&package(), "2"), shape_2());
    let mut s = open(&vsdx);
    let native = dir.path("a.diacraft");
    save_as(&mut s, &native);
    let mut s = open(&native);
    let out = dir.path("b.vsdx");
    save_as(&mut s, &out);
    assert_eq!(outline(&dir.read("b.vsdx"), "1"), shape_1());
    assert_eq!(outline(&dir.read("b.vsdx"), "2"), shape_2());
}

/// A `.diacraft` file, opened, changed, and saved as a VSDX file: the node stands after the row it followed.
#[test]
fn a_model_writer_puts_each_unknown_node_after_the_row_it_followed_in_a_diacraft_file_saved_as_vsdx() {
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

/// The same edit saved in the `.diacraft` file, which is then opened and saved as a VSDX file: the file holds the place as the session had it.
#[test]
fn a_diacraft_file_keeps_the_place_of_each_unknown_node_through_a_save_and_a_reopen() {
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
fn a_save_as_across_the_formats_keeps_the_place_of_each_unknown_node() {
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

/// A save into the VSDX file splices into it and keeps each node where the tree has it: the same outline, which is what the model writers are held to.
#[test]
fn a_save_into_a_vsdx_file_keeps_each_unknown_node_after_the_row_it_followed() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &package());
        let mut s = open(&vsdx);
        (case.run)(&mut s);
        s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{}: the save: {e}", case.name));
        check(case, &case.expect, &dir.read("a.vsdx"), "saved into the .vsdx", &mut failures);
    }
    report(failures, cases.len());
}

/// An undo puts every node back where it stood, in what a model writer makes. (A Save As makes the file the session's own, and what is saved after it is a splice into that file, so
/// each of these makes one save, at the end.)
#[test]
fn an_undo_puts_each_unknown_node_back_where_it_stood() {
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
        let before = if case.shape == "1" { shape_1() } else { shape_2() };
        check(case, &before, &dir.read("undone.vsdx"), "after the undo", &mut failures);
    }
    report(failures, cases.len());
}

/// A redo puts every node where the commands put it, in what a model writer makes.
#[test]
fn a_redo_puts_each_unknown_node_where_the_commands_put_it() {
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
        for _ in 0..case.steps {
            s.execute("edit.redo", &json!({})).unwrap_or_else(|e| panic!("{}: the redo: {e}", case.name));
        }
        let out = dir.path("redone.vsdx");
        save_as(&mut s, &out);
        check(case, &case.expect, &dir.read("redone.vsdx"), "after the redo", &mut failures);
    }
    report(failures, cases.len());
}

// ----------------------------------------------------------------------------------------------------------------------------------- costs

/// The unknown nodes of the section `name` of the shape `id` of the first page: the places they say they stand at.
fn places_in(s: &Session, id: u32, name: &str) -> Vec<u32> {
    let doc = s.document().unwrap();
    let shape = doc.pages.iter().next().and_then(|page| page.shapes.iter().find(|shape| shape.id == id)).unwrap();
    let section = shape.sheet.sections.iter().find(|section| section.name.as_str() == name).unwrap();
    section.passthrough.iter().map(|placed| placed.after).collect()
}

/// **Counted, not timed.** A copy of thousands of rows into an instance, and the insert after it, move the nodes of the instance's section in a few passes over the nodes (one for the
/// merge of the rows that were copied and one for the row put in), not one pass for each row copied; and the nodes end where the rows they followed went.
#[test]
fn a_copy_of_thousands_of_rows_into_an_instance_moves_the_nodes_of_its_section_in_a_few_passes() {
    const ROWS: usize = 5_000;
    let master_rows: String = (0..ROWS).map(|i| format!(r#"<Row IX="{i}"><Cell N="A" V="{i}"/></Row>"#)).collect();
    let master = format!(
        r#"<Shape ID="5" Type="Shape" NameU="Tmpl" Name="Tmpl"><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Section N="Scratch">{master_rows}</Section></Shape>"#
    );
    let instance = format!(
        r#"<Shape ID="2" Type="Shape" Master="1" NameU="Tmpl.2" Name="Tmpl.2"><Cell N="PinX" V="4"/><Cell N="PinY" V="4"/>
  <Section N="Scratch"><Row IX="100" K="a"><Cell N="A" V="1"/></Row><x:N1 {X}/><Row IX="2500" K="b"><Cell N="A" V="2"/></Row><x:N2 {X}/></Section></Shape>"#
    );
    let bytes = VsdxBuilder::new().master(MasterSpec::new(1, "Tmpl").shapes(&master)).page(PageSpec::new(1, "Page-1").shapes(&instance)).build();
    let dir = Dir::new();
    let mut s = open(&dir.write("big.vsdx", &bytes));
    assert_eq!(places_in(&s, 2, "Scratch"), [1, 2], "the nodes stand after the first row and after the second");
    diacraft_doc::work::reset();
    call(&mut s, "sheet.add_row", SHAPE_2, json!({"section": "Scratch", "after": 1}));
    let read = diacraft_doc::work::places_read();
    assert!(read <= 4 * 2, "the places of two nodes were read {read} times: a few passes over them, and not one for each of {ROWS} rows");
    // After the copy the first node follows the row `a` ahead of the rows put in after it (right before `b`, which is at 2500), and the second follows the last row; then the row
    // put in at the front moves both by one.
    assert_eq!(places_in(&s, 2, "Scratch"), [2501, 5001]);
}

/// **Counted, not timed.** A command that puts in or takes out a row reads the places of the nodes of the section once, however many rows it numbers again: a section of rows with a
/// node after each costs a pass over the nodes for the command, and not a pass for each row that moved.
#[test]
fn a_command_reads_the_nodes_of_a_section_in_one_pass_whatever_number_of_rows_it_numbers_again() {
    const ROWS: usize = 3_000;
    let rows: String = (0..ROWS).map(|i| format!(r#"<Row IX="{i}"><Cell N="A" V="{i}"/></Row><!--after row {i}-->"#)).collect();
    let shape = format!(
        r#"<Shape ID="1" Type="Shape" NameU="Box" Name="Box"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Section N="Scratch">{rows}</Section></Shape>"#
    );
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shape)).build();
    let dir = Dir::new();
    let mut s = open(&dir.write("many.vsdx", &bytes));
    assert_eq!(places_in(&s, 1, "Scratch").len(), ROWS);
    // A row before the first numbers every row again.
    diacraft_doc::work::reset();
    call(&mut s, "sheet.add_row", SHAPE_1, json!({"section": "Scratch", "before": 1}));
    assert!(diacraft_doc::work::places_read() <= 2 * ROWS as u64, "one pass over the {ROWS} nodes for the row put in");
    // A row taken out from the front numbers them all again.
    diacraft_doc::work::reset();
    call(&mut s, "sheet.delete_row", SHAPE_1, json!({"section": "Scratch", "row": 1}));
    assert!(diacraft_doc::work::places_read() <= 2 * ROWS as u64, "one pass over the {ROWS} nodes for the row taken out");
    // The two together are no change to a node: each stands after the row it followed.
    assert_eq!(places_in(&s, 1, "Scratch"), (1..=ROWS as u32).collect::<Vec<_>>());
}
