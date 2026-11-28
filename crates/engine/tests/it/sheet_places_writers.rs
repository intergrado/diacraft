//! **Every writer keeps the place of the unknown nodes**, not only the row and section commands of `sheet_places.rs`.
//!
//! An unknown node (a foreign element, a comment) is kept as the count of typed children that stood before it (`Placed::after`), and a model writer puts it back after that many.
//! Whoever puts a cell, a row or a section in, or takes one out, moves the count, or the node is written one place off in the file a model writer makes (a `.diacraft` file saved as
//! a VSDX file). This file holds the cases of the writers that make a cell, a row or a section as a side effect (the cell commands `sheet.set_cell` and `sheet.set_cells` on an instance
//! and on a shape, the writers of Shape Data, Hyperlinks, the text's formatting and the layers), each on a drawing whose sheets hold nodes among the cells, the rows and the sections.
//!
//! An outline lists the children of a shape (or of a page's sheet) in order: a cell as `c:Name`, a section as `Name[its rows and its unknown nodes]`, a row by its `K` (`new` for a
//! row the command made) with its own cells and nodes in braces when it holds a node, an unknown element as `?Name`, a comment as `#text` and the text as `Text`. A model writer writes
//! the cells of an element in the order of their names, so the cells of the drawing are named in that order.

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
<Shape ID="3" Type="Shape" NameU="Box" Name="Box">
  <Cell N="Height" V="1"/>
  <!-- after Height -->
  <Cell N="PinX" V="2"/>
  <x:AfterPinX {X}/>
  <Cell N="PinY" V="2"/>
  <Cell N="Width" V="1"/>
  <x:AfterCells {X}/>
  <Section N="Scratch">
    <x:Lead {X}/>
    <Row IX="0" K="r1"><Cell N="A" V="10"/><x:InRow {X}/></Row>
    <x:Hint {X}/>
    <Row IX="1" K="r2"><Cell N="A" V="20"/></Row>
    <x:Trail {X}/>
  </Section>
  <!-- between the sections -->
  <Section N="Property">
    <x:P0 {X}/>
    <Row N="Cost" K="p1"><Cell N="Label" V="Cost"/><Cell N="Prompt" V=""/><Cell N="Type" V="2"/><Cell N="Format" V=""/><Cell N="SortKey" V=""/><Cell N="Invisible" V="0"/><Cell N="Verify" V="0"/><Cell N="DataLinked" V="0"/><Cell N="LangID" V="1033"/><Cell N="Calendar" V="0"/><Cell N="Value" V="12.5"/></Row>
    <x:P1 {X}/>
    <Row N="Owner" K="p2"><Cell N="Label" V="Owner"/><Cell N="Prompt" V=""/><Cell N="Type" V="0"/><Cell N="Format" V=""/><Cell N="SortKey" V=""/><Cell N="Invisible" V="0"/><Cell N="Verify" V="0"/><Cell N="DataLinked" V="0"/><Cell N="LangID" V="1033"/><Cell N="Calendar" V="0"/><Cell N="Value" V="Ann"/></Row>
    <x:P2 {X}/>
  </Section>
  <Section N="Hyperlink">
    <x:H0 {X}/>
    <Row N="Row_1" K="h1"><Cell N="Description" V="One"/><Cell N="Address" V="https://one.example/"/><Cell N="SubAddress" V=""/><Cell N="Frame" V=""/></Row>
    <x:H1 {X}/>
    <Row N="Row_2" K="h2"><Cell N="Description" V="Two"/><Cell N="Address" V="https://two.example/"/><Cell N="SubAddress" V=""/><Cell N="Frame" V=""/></Row>
    <x:H2 {X}/>
  </Section>
  <Section N="Character">
    <x:C0 {X}/>
    <Row IX="0" K="c1"><Cell N="Size" V="0.1667"/></Row>
    <x:C1 {X}/>
  </Section>
  <x:AfterSections {X}/>
  <Text><cp IX="0"/>Hello World</Text>
  <x:Foreign {X}/>
</Shape>
<Shape ID="4" Type="Shape" Master="1" NameU="Tmpl.4" Name="Tmpl.4">
  <Cell N="PinX" V="4"/>
  <x:AfterPinX4 {X}/>
  <Cell N="PinY" V="4"/>
  <Section N="Scratch">
    <x:Lead4 {X}/>
    <Row IX="1" K="i1"><Cell N="A" V="22"/></Row>
    <x:Note4 {X}/>
  </Section>
  <x:Tail4 {X}/>
</Shape>
<Shape ID="6" Type="Shape" Master="1" NameU="Tmpl.6" Name="Tmpl.6">
  <Cell N="PinX" V="6"/>
  <Section N="Scratch">
    <x:Lead6 {X}/>
    <Row IX="2" K="d2"><Cell N="A" V="32"/></Row>
    <x:Mid6 {X}/>
    <Row IX="1" K="d1"><Cell N="A" V="31"/></Row>
    <x:Note6 {X}/>
  </Section>
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
  <Section N="User">
    <Row N="Rate"><Cell N="Value" V="5"/></Row>
  </Section>
</Shape>"#;

fn page_sheet() -> String {
    format!(
        r#"<Cell N="PageWidth" V="8.5"/><x:AfterPageWidth {X}/>
  <Section N="Layer">
    <x:L0 {X}/>
    <Row IX="0" K="l0"><Cell N="Name" V="Zero"/></Row>
    <x:L1 {X}/>
    <Row IX="1" K="l1"><Cell N="Name" V="One"/></Row>
    <x:L2 {X}/>
    <Row IX="2" K="l2"><Cell N="Name" V="Two"/></Row>
    <x:L3 {X}/>
  </Section>"#
    )
}

/// The drawing as a VSDX file.
fn package() -> Vec<u8> {
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "Tmpl").shapes(MASTER_SHAPE))
        .page(PageSpec::new(1, "Page-1").sheet(&page_sheet()).shapes(&shapes()))
        .build()
}

// ------------------------------------------------------------------------------------------------------------------------------ outlines

/// What stands for a row in an outline: its `K`, else `new`, and its own children when it holds a node.
fn row_outline(row: &El) -> String {
    let label = row.attr("K").unwrap_or("new").to_owned();
    let has_node =
        row.children.iter().any(|n| !matches!(n, Node::El(c) if c.is("Cell")) && !matches!(n, Node::Text(_) | Node::CData(_) | Node::Pi { .. }));
    if !has_node {
        return label;
    }
    let inner: Vec<String> = children(row);
    format!("{label}{{{}}}", inner.join(","))
}

/// The children of an element that an outline lists: the cells, the rows (in a section), the sections, the text, the unknown elements and the comments.
fn children(el: &El) -> Vec<String> {
    let mut out = Vec::new();
    for node in &el.children {
        match node {
            Node::El(child) if child.is("Cell") => out.push(format!("c:{}", child.attr("N").unwrap_or(""))),
            Node::El(child) if child.is("Row") => out.push(row_outline(child)),
            Node::El(child) if child.is("Section") => {
                let inner: Vec<String> = children(child).into_iter().filter(|c| !c.starts_with("c:")).collect();
                out.push(format!("{}[{}]", child.attr("N").unwrap_or(""), inner.join(",")));
            }
            Node::El(child) if child.is("Text") => out.push("Text".to_owned()),
            Node::El(child) => out.push(format!("?{}", child.local())),
            Node::Comment(text) => out.push(format!("#{}", text.trim())),
            Node::Text(_) | Node::CData(_) | Node::Pi { .. } => {}
        }
    }
    out
}

/// The tail of the name of a part: the part is found by it among the parts of the package, whatever folder the package keeps its parts in.
fn part<'a>(parts: &'a std::collections::BTreeMap<String, Vec<u8>>, tail: &str) -> &'a [u8] {
    parts.iter().find(|(name, _)| name.ends_with(tail)).map(|(_, bytes)| bytes.as_slice()).unwrap_or_else(|| panic!("the package has no part {tail}"))
}

#[derive(Clone, Copy, Debug)]
enum Subject {
    /// The shape with this `ID` on the first page.
    Shape(&'static str),
    /// The first page's `PageSheet`.
    PageSheet,
}

/// The outline of a subject in a package.
fn outline(package: &[u8], subject: Subject) -> Vec<String> {
    let parts = parts_of(package);
    match subject {
        Subject::Shape(id) => {
            let root = diacraft_opc::xml::parse(part(&parts, "/pages/page1.xml")).unwrap();
            let shape = root
                .elements()
                .filter(|e| e.is("Shapes"))
                .flat_map(|shapes| shapes.elements())
                .find(|s| s.is("Shape") && s.attr("ID") == Some(id))
                .unwrap_or_else(|| panic!("no shape {id} in the page"));
            children(shape)
        }
        Subject::PageSheet => {
            let root = diacraft_opc::xml::parse(part(&parts, "/pages/pages.xml")).unwrap();
            let sheet = root
                .elements()
                .filter(|e| e.is("Page"))
                .flat_map(|page| page.elements())
                .find(|e| e.is("PageSheet"))
                .unwrap_or_else(|| panic!("no PageSheet in the pages index"));
            children(sheet)
        }
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

// -------------------------------------------------------------------------------------------------------------------------------- cases

fn call(s: &mut Session, command: &str, params: Value) {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
}

struct Case {
    name: &'static str,
    /// The commands, and how many there are (each is one undo step).
    run: fn(&mut Session),
    steps: usize,
    subject: Subject,
    expect: Vec<String>,
    /// Whether a save into the VSDX file (a splice) is held to the same outline: the splice puts a cell where the file's own order says, which a writer that sorts the cells does not.
    splice: bool,
}

const SHAPE_3: Subject = Subject::Shape("3");
const SHAPE_4: Subject = Subject::Shape("4");
const SHAPE_6: Subject = Subject::Shape("6");

fn cases() -> Vec<Case> {
    let mut list = Vec::new();
    let mut case = |name: &'static str, steps: usize, run: fn(&mut Session), subject: Subject, splice: bool, expect: &[&str]| {
        list.push(Case { name, run, steps, subject, expect: words(expect), splice });
    };
    // ---- the cell commands, on an instance: a cell written into a row or a section its master gives makes the row and the section the instance holds.
    case(
        "an instance: a cell of a row its master gives makes the row after the rows it holds",
        1,
        |s| call(s, "sheet.set_cell", json!({"id": 4, "cell": "Scratch.A1", "formula": "7"})),
        SHAPE_4,
        true,
        &["c:PinX", "?AfterPinX4", "c:PinY", "Scratch[?Lead4,i1,new,?Note4]", "?Tail4"],
    );
    case(
        "an instance: a cell of a section its master gives makes the section ahead of the node that trailed the sections",
        1,
        |s| call(s, "sheet.set_cell", json!({"id": 4, "cell": "User.Rate", "formula": "3"})),
        SHAPE_4,
        true,
        &["c:PinX", "?AfterPinX4", "c:PinY", "Scratch[?Lead4,i1,?Note4]", "User[new]", "?Tail4"],
    );
    case(
        "an instance: a cell it did not hold, which sorts before the cells it holds",
        1,
        |s| call(s, "sheet.set_cell", json!({"id": 4, "cell": "Angle", "formula": "30 deg"})),
        SHAPE_4,
        false,
        &["c:Angle", "c:PinX", "?AfterPinX4", "c:PinY", "Scratch[?Lead4,i1,?Note4]", "?Tail4"],
    );
    case(
        "an instance: a cell it did not hold, which sorts between the cells it holds",
        1,
        |s| call(s, "sheet.set_cell", json!({"id": 4, "cell": "LocPinX", "formula": "1"})),
        SHAPE_4,
        false,
        &["c:LocPinX", "c:PinX", "?AfterPinX4", "c:PinY", "Scratch[?Lead4,i1,?Note4]", "?Tail4"],
    );
    case(
        "an instance: a batch of writes that make a cell, a row and a section",
        1,
        |s| {
            call(
                s,
                "sheet.set_cells",
                json!({"writes": [
                    {"id": 4, "cell": "Angle", "formula": "30 deg"},
                    {"id": 4, "cell": "Scratch.A1", "formula": "7"},
                    {"id": 4, "cell": "User.Rate", "formula": "3"},
                ]}),
            );
        },
        SHAPE_4,
        false,
        &["c:Angle", "c:PinX", "?AfterPinX4", "c:PinY", "Scratch[?Lead4,i1,new,?Note4]", "User[new]", "?Tail4"],
    );
    // ---- a copy into an instance of a section whose own rows are out of order (a file another program wrote lists the rows of a section by descending numbers): the rows are
    // put in order whole, and **each node stays with the row it followed** (it used to keep its count and stand after another row).
    case(
        "an instance whose own rows are out of order: an insert in the middle of what it inherits copies the master's rows and puts the rows in order, the nodes with their rows",
        1,
        |s| call(s, "sheet.add_row", json!({"id": 6, "section": "Scratch", "after": 1})),
        SHAPE_6,
        false,
        &["c:PinX", "Scratch[?Lead6,new,new,d1,?Note6,d2,?Mid6]"],
    );
    // ---- the cell commands, on a shape that holds the cells: a cell it did not hold goes among them, and a cell a row did not hold goes among the row's.
    case(
        "a shape: a cell that sorts before the first",
        1,
        |s| call(s, "sheet.set_cell", json!({"id": 3, "cell": "Angle", "formula": "30 deg"})),
        SHAPE_3,
        false,
        &[
            "c:Angle",
            "c:Height",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r1{c:A,?InRow},?Hint,r2,?Trail]",
            "#between the sections",
            "Property[?P0,p1,?P1,p2,?P2]",
            "Hyperlink[?H0,h1,?H1,h2,?H2]",
            "Character[?C0,c1,?C1]",
            "?AfterSections",
            "Text",
            "?Foreign",
        ],
    );
    case(
        "a shape: a cell that sorts between two",
        1,
        |s| call(s, "sheet.set_cell", json!({"id": 3, "cell": "LocPinX", "formula": "1"})),
        SHAPE_3,
        false,
        &[
            "c:Height",
            "c:LocPinX",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r1{c:A,?InRow},?Hint,r2,?Trail]",
            "#between the sections",
            "Property[?P0,p1,?P1,p2,?P2]",
            "Hyperlink[?H0,h1,?H1,h2,?H2]",
            "Character[?C0,c1,?C1]",
            "?AfterSections",
            "Text",
            "?Foreign",
        ],
    );
    case(
        "a shape: a cell a row did not hold goes among the row's cells, ahead of the node that followed the cell before it",
        1,
        |s| call(s, "sheet.set_cell", json!({"id": 3, "cell": "Scratch.B1", "formula": "5"})),
        SHAPE_3,
        false,
        &[
            "c:Height",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r1{c:A,c:B,?InRow},?Hint,r2,?Trail]",
            "#between the sections",
            "Property[?P0,p1,?P1,p2,?P2]",
            "Hyperlink[?H0,h1,?H1,h2,?H2]",
            "Character[?C0,c1,?C1]",
            "?AfterSections",
            "Text",
            "?Foreign",
        ],
    );
    // ---- the writers that go through a section or a row of their own.
    case(
        "shapedata.define: a row left out goes with its node, a row put in goes ahead of the node that trailed the rows",
        1,
        |s| call(s, "shapedata.define", json!({"id": 3, "fields": [{"row": "Owner"}, {"label": "Region", "name": "Region"}]})),
        SHAPE_3,
        true,
        &[
            "c:Height",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r1{c:A,?InRow},?Hint,r2,?Trail]",
            "#between the sections",
            "Property[?P0,?P1,p2,new,?P2]",
            "Hyperlink[?H0,h1,?H1,h2,?H2]",
            "Character[?C0,c1,?C1]",
            "?AfterSections",
            "Text",
            "?Foreign",
        ],
    );
    case(
        "hyperlink.set: a row left out goes with its node, a row put in goes ahead of the node that trailed the rows",
        1,
        |s| {
            call(
                s,
                "hyperlink.set",
                json!({"id": 3, "links": [{"name": "Row_2", "address": "https://two.example/"}, {"description": "Three", "address": "https://three.example/"}]}),
            );
        },
        SHAPE_3,
        true,
        &[
            "c:Height",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r1{c:A,?InRow},?Hint,r2,?Trail]",
            "#between the sections",
            "Property[?P0,p1,?P1,p2,?P2]",
            "Hyperlink[?H0,?H1,h2,new,?H2]",
            "Character[?C0,c1,?C1]",
            "?AfterSections",
            "Text",
            "?Foreign",
        ],
    );
    case(
        "hyperlink.set with no link: the section goes, and the nodes of the sheet that followed it follow the section before",
        1,
        |s| call(s, "hyperlink.set", json!({"id": 3, "links": []})),
        SHAPE_3,
        true,
        &[
            "c:Height",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r1{c:A,?InRow},?Hint,r2,?Trail]",
            "#between the sections",
            "Property[?P0,p1,?P1,p2,?P2]",
            "Character[?C0,c1,?C1]",
            "?AfterSections",
            "Text",
            "?Foreign",
        ],
    );
    case(
        "text.format of a part of the text: the rows it makes go ahead of the node that trailed the rows",
        1,
        |s| call(s, "text.format", json!({"ids": [3], "start": 0, "end": 5, "bold": true})),
        SHAPE_3,
        false,
        &[
            "c:Height",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r1{c:A,?InRow},?Hint,r2,?Trail]",
            "#between the sections",
            "Property[?P0,p1,?P1,p2,?P2]",
            "Hyperlink[?H0,h1,?H1,h2,?H2]",
            "Character[?C0,c1,new,?C1]",
            "?AfterSections",
            "Text",
            "?Foreign",
        ],
    );
    case(
        "layer.delete: the row goes with the nodes that followed it, and the rows after it close up",
        1,
        |s| call(s, "layer.delete", json!({"layer": 1})),
        Subject::PageSheet,
        true,
        &["c:PageWidth", "?AfterPageWidth", "Layer[?L0,l0,?L1,?L2,l2,?L3]"],
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
    let seen = outline(got, case.subject);
    if seen != expect {
        failures.push(format!("[{what}] {}:\n      want {expect:?}\n      got  {seen:?}", case.name));
    }
}

fn report(failures: Vec<String>, total: usize) {
    assert!(failures.is_empty(), "{} of {total} trips failed:\n{}", failures.len(), failures.join("\n"));
}

/// The drawing is what its outline says before any command, in the file and in the one a model writer makes of it.
#[test]
fn the_drawing_of_the_writers_stands_as_its_outline_says_in_the_file_and_in_what_a_model_writer_makes_of_it() {
    let dir = Dir::new();
    let vsdx = dir.write("a.vsdx", &package());
    let mut s = open(&vsdx);
    let native = dir.path("a.diacraft");
    save_as(&mut s, &native);
    let mut s = open(&native);
    let out = dir.path("b.vsdx");
    save_as(&mut s, &out);
    for subject in [SHAPE_3, SHAPE_4, Subject::PageSheet] {
        assert_eq!(outline(&dir.read("b.vsdx"), subject), outline(&package(), subject), "{subject:?}");
    }
    assert_eq!(outline(&package(), SHAPE_4), words(&["c:PinX", "?AfterPinX4", "c:PinY", "Scratch[?Lead4,i1,?Note4]", "?Tail4"]));
}

/// A `.diacraft` file, opened, changed, and saved as a VSDX file: the node stands after the child it followed.
#[test]
fn a_model_writer_puts_each_unknown_node_after_the_child_it_followed_whichever_writer_changed_the_children() {
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
fn a_diacraft_file_keeps_the_place_of_each_node_whichever_writer_changed_the_children() {
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
fn a_save_as_across_the_formats_keeps_the_place_of_each_node_whichever_writer_changed_the_children() {
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
fn a_save_into_a_vsdx_file_keeps_each_node_after_the_child_it_followed_whichever_writer_changed_the_children() {
    let mut failures = Vec::new();
    let cases: Vec<Case> = cases().into_iter().filter(|case| case.splice).collect();
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

/// An undo puts every node back where it stood, in what a model writer makes.
#[test]
fn an_undo_puts_each_node_back_where_it_stood_whichever_writer_changed_the_children() {
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
        check(case, &outline(&package(), case.subject), &dir.read("undone.vsdx"), "after the undo", &mut failures);
    }
    report(failures, cases.len());
}

/// A redo puts every node where the commands put it, in what a model writer makes.
#[test]
fn a_redo_puts_each_node_where_the_commands_put_it_whichever_writer_changed_the_children() {
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
