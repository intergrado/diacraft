//! **A list that a command makes again keeps each unknown node after the child it followed**: the route a connector is given, the path a tool carries on, the sheet of
//! a shape that is changed to another master.
//!
//! An unknown node (a foreign element, a comment) is kept as the count of typed children that stood before it (`Placed::after`), and a model writer puts it back after that many
//! (`sheet_places.rs` and `sheet_places_writers.rs` have the rest of the writers). A command that puts the rows of a `Geometry` section in place of its own, or builds the sheet
//! of a shape again, has to move the count with the children, or the node stands one place off in the file a model writer makes (a `.diacraft` file saved as a VSDX file). **A node goes
//! on following the child it followed** (a row told by its `IX`, a cell by its name, a section by its name and `IX`), **a node whose child is gone goes with the child before it, a child
//! that is new goes ahead of the nodes that stood after the child before it, and a node before the first child stays before it.**
//!
//! An outline lists the children of the first `Geometry` section of a shape in order, or those of the whole shape for an `ID` written with a `*` ahead of it: a cell as `c:Name`, a row as
//! `r<IX>`, a section as `Name<IX>[its children]`, an unknown element as `?Name` and a comment as `#text`. The last cases are **a Save As an `.odg`** (`odgfile/flatten.rs`): the copy of a
//! shape the file is given holds the frame and the style as constants, the geometry its master gives and the sections in another order, and the VSDX file that comes back from the `.odg`
//! holds each node after the same cell, row or section. The costs are counted, not timed: the keys and the nodes are read once each, and nothing is read for a list that holds no node.

use diacraft_container::xml::{El, Node};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::sheet_files_world::parts_of;

const X: &str = r#"xmlns:x="urn:test:unknown""#;

/// The page's size, which a Save As `.odg` needs of the first page.
const PAGE: &str = r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/>"#;

// ------------------------------------------------------------------------------------------------------------------------------ drawings

/// A master of a 2 by 1 box with a connection point on its right (row 0) and on its left (row 1), as a file writes it.
fn master_shape() -> String {
    shape(
        4,
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Connection">
<Row IX="0"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row>
<Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row></Section>"#,
    )
}

/// The rows of a route of `rows` rows from the begin of the connector to its end, each of its two cells a constant (a file's own route), with the unknown nodes `nodes` put as they are.
fn route_rows(rows: u32, nodes: &str) -> String {
    let mut out = String::new();
    for ix in 1..=rows {
        let kind = if ix == 1 { "MoveTo" } else { "LineTo" };
        out.push_str(&format!(r#"<Row T="{kind}" IX="{ix}"><Cell N="X" V="{ix}"/><Cell N="Y" V="0"/></Row>"#));
        out.push_str(&nodes.replace("{ix}", &ix.to_string()));
    }
    out
}

/// Instance 10 at (2, 2), instance 11, three wide, at (6, 2) and connector 12 joining them with the formulas and `Connect` rows a file holds. **The connector's own `Geometry` section holds
/// unknown nodes among its cell and its rows**: one before the cell, one after it (none when `bare`), and the `nodes` after each of the `rows` rows of its route.
fn route_shapes(rows: u32, nodes: &str, bare: bool) -> String {
    let a = shape_with(10, r#" Master="7""#, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#);
    let b = shape_with(
        11,
        r#" Master="7""#,
        r#"<Cell N="PinX" V="6"/><Cell N="PinY" V="2"/><Cell N="Width" V="3"/><Cell N="LocPinX" V="1.5" F="Inh"/>
<Section N="Connection"><Row IX="0"><Cell N="X" V="3" F="Inh"/></Row></Section>"#,
    );
    let c = shape(
        12,
        &format!(
            r#"<Cell N="BeginX" V="3" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/><Cell N="BeginY" V="2" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/>
<Cell N="EndX" V="4.5" F="PAR(PNT(Sheet.11!Connections.X2,Sheet.11!Connections.Y2))"/><Cell N="EndY" V="2" F="PAR(PNT(Sheet.11!Connections.X2,Sheet.11!Connections.Y2))"/>
<Cell N="PinX" V="3.75" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="1.5" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="0.75" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Section N="Geometry" IX="0">{lead}<Cell N="NoFill" V="1"/>{trail}{route}</Section>"#,
            lead = if bare { String::new() } else { format!("<x:G0 {X}/>") },
            trail = if bare { String::new() } else { format!("<x:G1 {X}/>") },
            route = route_rows(rows, nodes),
        ),
    );
    format!("{a}{b}{c}")
}

fn connects() -> &'static str {
    r#"<Connect FromSheet="12" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Connections.X1" ToPart="100"/>
<Connect FromSheet="12" FromCell="EndX" FromPart="12" ToSheet="11" ToCell="Connections.X2" ToPart="101"/>"#
}

fn route_package_with(rows: u32, nodes: &str, bare: bool) -> Vec<u8> {
    VsdxBuilder::new()
        .master(MasterSpec::new(7, "Box").shapes(&master_shape()))
        .page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&route_shapes(rows, nodes, bare)).connects(connects()))
        .build()
}

fn route_package(rows: u32, nodes: &str) -> Vec<u8> {
    route_package_with(rows, nodes, false)
}

/// A route of two rows, an element after each.
fn short_route() -> Vec<u8> {
    let nodes = format!(r#"<x:after{{ix}} {X}/>"#);
    route_package(2, &nodes)
}

/// A route of six rows, an element after each.
fn long_route() -> Vec<u8> {
    let nodes = format!(r#"<x:after{{ix}} {X}/>"#);
    route_package(6, &nodes)
}

/// A shape that holds a node among its cells, its sections and their rows, which `shape.change` gives another master.
fn change_package() -> Vec<u8> {
    let boxed = shape_with(
        3,
        r#" Type="Shape" NameU="Box" Name="Box""#,
        &format!(
            r##"<Cell N="Height" V="1"/>
<!-- after Height -->
<Cell N="PinX" V="2"/>
<x:AfterPinX {X}/>
<Cell N="PinY" V="2"/>
<Cell N="Width" V="1"/>
<x:AfterCells {X}/>
<Section N="Scratch"><x:Lead {X}/><Row IX="0"><Cell N="A" V="10"/></Row><x:Hint {X}/><Row IX="1"><Cell N="A" V="20"/></Row><x:Trail {X}/></Section>
<!-- between the sections -->
<Section N="Geometry" IX="0"><x:G0 {X}/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><x:G1 {X}/><Row T="LineTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row><x:G2 {X}/></Section>
<Section N="Hyperlink"><x:H0 {X}/><Row N="Row_1"><Cell N="Address" V="https://one.example/"/></Row><x:H1 {X}/></Section>
<x:AfterSections {X}/>
<Text><cp IX="0"/>Hello World</Text>"##
        ),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&boxed)).build()
}

/// An instance of a master of two inches by two, which holds a node among its own cells and after its section, and which `shape.change` gives another master: the sheet it comes to
/// holds the frame cells the old master supplied, which it did not hold, among the cells it did.
fn instance_package() -> Vec<u8> {
    let master = shape(
        5,
        r#"<Cell N="Width" V="2"/><Cell N="Height" V="2"/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="2"/></Row></Section>"#,
    );
    let instance = shape_with(
        4,
        r#" Type="Shape" Master="1" NameU="Tmpl.4" Name="Tmpl.4""#,
        &format!(
            r#"<x:Lead {X}/><Cell N="PinX" V="4"/><x:AfterPinX {X}/><Cell N="PinY" V="4"/><x:AfterPinY {X}/>
<Section N="Scratch"><x:SLead {X}/><Row IX="0"><Cell N="A" V="1"/></Row><x:STrail {X}/></Section><x:AfterSections {X}/>"#
        ),
    );
    VsdxBuilder::new().master(MasterSpec::new(1, "Tmpl").shapes(&master)).page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&instance)).build()
}

/// An instance of a master that holds a text and the first row of its `Character` section, whose own `Character` section holds the rows 1 and 2 with a node among them: a copy of it for an
/// `.odg` holds the master's row 0 as the shape's own, ahead of the rows it held (`section_over`).
fn text_package() -> Vec<u8> {
    let master = shape(
        5,
        r#"<Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Section N="Character"><Row IX="0"><Cell N="Size" V="0.2"/></Row></Section><Text><cp IX="0"/>Hi</Text>"#,
    );
    let instance = shape_with(
        4,
        r#" Type="Shape" Master="1" NameU="Tmpl.4" Name="Tmpl.4""#,
        &format!(
            r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="4"/>
<Section N="Character"><x:CLead {X}/><Row IX="1"><Cell N="Size" V="0.3"/></Row><x:CMid {X}/><Row IX="2"><Cell N="Size" V="0.4"/></Row><x:CTrail {X}/></Section>"#
        ),
    );
    VsdxBuilder::new().master(MasterSpec::new(1, "Tmpl").shapes(&master)).page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&instance)).build()
}

/// A guide with a node among its cells: the copy an `.odg` save is given holds the two pattern cells a guide has none of, which go in among the cells it held.
fn guide_package() -> Vec<u8> {
    let guide = shape_with(
        9,
        r#" Type="Guide" NameU="Guide" Name="Guide""#,
        &format!(r#"<x:Lead {X}/><Cell N="PinX" V="2"/><x:AfterPinX {X}/><Cell N="PinY" V="0"/><x:AfterPinY {X}/>"#),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&guide)).build()
}

/// A group of a member with nodes among the group's cells and sections, among the members and among the member's cells.
fn group_package() -> Vec<u8> {
    let group = shape_with(
        6,
        r#" Type="Group" NameU="Grp" Name="Grp""#,
        &format!(
            r#"<x:Lead {X}/><Cell N="Height" V="2"/><x:AfterHeight {X}/><Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="1" F="Height*0.5"/><Cell N="PinX" V="3"/><Cell N="PinY" V="3"/><x:AfterPinY {X}/><Cell N="Width" V="2"/>
<Section N="User"><x:ULead {X}/><Row N="Note"><Cell N="Value" V="1"/></Row><x:UTrail {X}/></Section><x:AfterSections {X}/>
<Shapes><x:M0 {X}/>{member}<x:M1 {X}/></Shapes>"#,
            member = shape_with(
                7,
                r#" Type="Shape" NameU="In" Name="In""#,
                &format!(
                    r#"<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Cell N="PinX" V="1"/><x:AfterMemberPinX {X}/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#
                )
            ),
        ),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&group)).build()
}

/// A shape with an open path of three rows, an element after the cell and after each row, which is closed by a command.
fn pen_package() -> Vec<u8> {
    let pen = shape_with(
        20,
        r#" Type="Shape" NameU="Pen" Name="Pen""#,
        &format!(
            r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="3"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><x:P0 {X}/><Cell N="NoFill" V="1"/><x:P1 {X}/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><!-- after one -->
<Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row><x:P2 {X}/>
<Row T="LineTo" IX="3"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row><x:P3 {X}/></Section>"#
        ),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(PAGE).shapes(&pen)).build()
}

// ------------------------------------------------------------------------------------------------------------------------------ outlines

/// The children of an element that an outline lists.
fn children(el: &El) -> Vec<String> {
    let mut out = Vec::new();
    for node in &el.children {
        match node {
            Node::El(child) if child.is("Cell") => out.push(format!("c:{}", child.attr("N").unwrap_or(""))),
            Node::El(child) if child.is("Row") => {
                let label = child.attr("IX").or_else(|| child.attr("N")).unwrap_or("?");
                out.push(format!("r{label}"));
            }
            Node::El(child) if child.is("Section") => {
                let ix = child.attr("IX").unwrap_or("");
                out.push(format!("{}{ix}[{}]", child.attr("N").unwrap_or(""), children(child).join(",")));
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

/// The outline of a shape of the first page of a package: the children of its `Geometry` section 0, or, for an `ID` written with a `*` ahead of it (`*3`), the children of the shape.
fn outline(package: &[u8], id: &str) -> Vec<String> {
    let (whole, id) = id.strip_prefix('*').map_or((false, id), |id| (true, id));
    let parts = parts_of(package);
    let root = diacraft_opc::xml::parse(part(&parts, "/pages/page1.xml")).unwrap();
    let shape = root
        .elements()
        .filter(|e| e.is("Shapes"))
        .flat_map(|shapes| shapes.elements())
        .find(|s| s.is("Shape") && s.attr("ID") == Some(id))
        .unwrap_or_else(|| {
            let held: Vec<&str> =
                root.elements().filter(|e| e.is("Shapes")).flat_map(|shapes| shapes.elements()).filter_map(|s| s.attr("ID")).collect();
            panic!("no shape {id} in the page, which holds {held:?}")
        });
    if whole {
        return children(shape);
    }
    let section = shape
        .elements()
        .find(|e| e.is("Section") && e.attr("N") == Some("Geometry") && e.attr("IX").unwrap_or("0") == "0")
        .unwrap_or_else(|| panic!("no Geometry section in shape {id}"));
    children(section)
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
    package: fn() -> Vec<u8>,
    run: fn(&mut Session),
    steps: usize,
    /// The `ID` of the shape the outline is of.
    shape: &'static str,
    expect: Vec<String>,
    /// Whether a save into the VSDX file (a splice) is held to the same outline.
    splice: bool,
}

/// The drawing's shape before any command.
fn before(case: &Case) -> Vec<String> {
    outline(&(case.package)(), case.shape)
}

fn cases() -> Vec<Case> {
    let mut list = Vec::new();
    let mut case =
        |name: &'static str, package: fn() -> Vec<u8>, steps: usize, run: fn(&mut Session), shape: &'static str, splice: bool, expect: &[&str]| {
            list.push(Case { name, package, run, steps, shape, expect: words(expect), splice });
        };
    // ---- the route of a connector: its rows are put in place of the rows the file's connector held.
    case(
        "a route of two rows made again as four: the rows that stay keep their nodes, and the new rows go ahead of the node that trailed the route",
        short_route,
        1,
        |s| call(s, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 1.0})),
        "12",
        true,
        &["?G0", "c:NoFill", "?G1", "r1", "?after1", "r2", "r3", "r4", "?after2"],
    );
    case(
        "a route of six rows made again as four: the nodes that followed the rows taken out go with the last row that stays",
        long_route,
        1,
        |s| call(s, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 1.0})),
        "12",
        true,
        &["?G0", "c:NoFill", "?G1", "r1", "?after1", "r2", "?after2", "r3", "?after3", "r4", "?after4", "?after5", "?after6"],
    );
    // ---- the path a tool carries on: the rows are made again from the shape's own.
    case(
        "a path closed: the row that closes it goes ahead of the node that trailed the rows, and the nodes among the others stay",
        pen_package,
        1,
        |s| call(s, "shape.close", json!({"id": 20})),
        "20",
        false,
        &["?P0", "c:NoFill", "?P1", "r1", "#after one", "r2", "?P2", "r3", "r4", "?P3"],
    );
    // ---- the sheet of a shape changed to another master: made again from the old shape's own cells and sections and the new master's.
    case(
        "shape.change of an instance: the frame cells the old master gave are written among the cells it held, each node stays after the cell it followed",
        instance_package,
        1,
        |s| call(s, "shape.change", json!({"ids": [4], "master": "ellipse"})),
        "*4",
        false,
        &[
            "?Lead",
            "c:Height",
            "c:LocPinX",
            "c:LocPinY",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:TxtHeight",
            "c:TxtLocPinX",
            "c:TxtLocPinY",
            "c:TxtPinX",
            "c:TxtPinY",
            "c:TxtWidth",
            "c:Width",
            "?AfterPinY",
            "Scratch[?SLead,r0,?STrail]",
            "Geometry0[r1]",
            "Connection[r0,r2,r3,r1]",
            "?AfterSections",
        ],
    );
    case(
        "shape.change: the sheet is made again, and each node stays after the cell or the section it followed",
        change_package,
        1,
        |s| call(s, "shape.change", json!({"ids": [3], "master": "ellipse"})),
        "*3",
        false,
        &[
            "c:Height",
            "c:LocPinX",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:TxtLocPinX",
            "c:TxtPinX",
            "c:TxtWidth",
            "c:Width",
            "?AfterCells",
            "Scratch[?Lead,r0,?Hint,r1,?Trail]",
            "#between the sections",
            "Hyperlink[?H0,rRow_1,?H1]",
            "Geometry0[r1]",
            "Connection[r0,r2,r3]",
            "?AfterSections",
            "Text",
        ],
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

/// The drawings stand as their outlines say before any command, in the file and in the one a model writer makes of it.
#[test]
fn the_drawings_stand_as_the_outlines_say_in_the_file_and_in_what_a_model_writer_makes_of_them() {
    for case in cases() {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &(case.package)());
        let mut s = open(&vsdx);
        let native = dir.path("a.diacraft");
        save_as(&mut s, &native);
        let mut s = open(&native);
        let out = dir.path("b.vsdx");
        save_as(&mut s, &out);
        assert_eq!(outline(&dir.read("b.vsdx"), case.shape), before(&case), "{}", case.name);
    }
    assert_eq!(before(&cases()[0]), words(&["?G0", "c:NoFill", "?G1", "r1", "?after1", "r2", "?after2"]));
    assert_eq!(before(&cases()[2]), words(&["?P0", "c:NoFill", "?P1", "r1", "#after one", "r2", "?P2", "r3", "?P3"]));
}

/// A `.diacraft` file, opened, changed, and saved as a VSDX file: the node stands after the child it followed.
#[test]
fn a_model_writer_puts_each_unknown_node_after_the_child_it_followed_in_a_diacraft_file_saved_as_vsdx() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &(case.package)());
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
fn a_diacraft_file_keeps_the_place_of_each_node_through_a_save_and_a_reopen() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &(case.package)());
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
fn a_save_as_across_the_formats_keeps_the_place_of_each_node() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &(case.package)());
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
fn a_save_into_a_vsdx_file_keeps_each_node_after_the_child_it_followed() {
    let mut failures = Vec::new();
    let cases: Vec<Case> = cases().into_iter().filter(|case| case.splice).collect();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &(case.package)());
        let mut s = open(&vsdx);
        (case.run)(&mut s);
        s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{}: the save: {e}", case.name));
        check(case, &case.expect, &dir.read("a.vsdx"), "saved into the .vsdx", &mut failures);
    }
    report(failures, cases.len());
}

// ----------------------------------------------------------------------------------------------------------------------------------- costs

/// **Counted, not timed.** A route made again for a connector whose `Geometry` section holds a node after each of its rows reads the keys of the rows once each and the nodes once; a
/// connector that holds no node reads neither (the section is not even copied).
#[test]
fn a_route_made_again_reads_the_keys_of_the_rows_and_the_nodes_once_each_and_a_section_with_no_node_reads_none() {
    const ROWS: u32 = 300;
    let dir = Dir::new();
    let nodes = format!(r#"<x:after{{ix}} {X}/>"#);
    let mut s = open(&dir.write("many.vsdx", &route_package(ROWS, &nodes)));
    diacraft_doc::work::reset();
    call(&mut s, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 1.0}));
    let (keys, read) = (diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read());
    assert!(keys > 0, "the rows were told apart by their keys");
    assert!(keys <= u64::from(ROWS) + 4 + 16, "the keys of the {ROWS} old rows and the 4 new ones were read {keys} times");
    assert!(read <= u64::from(ROWS) + 2 + 16, "the {} nodes were read {read} times for one route", ROWS + 2);
    let mut plain = open(&dir.write("none.vsdx", &route_package_with(ROWS, "", true)));
    diacraft_doc::work::reset();
    call(&mut plain, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 1.0}));
    assert_eq!((diacraft_doc::work::follow_keys(), diacraft_doc::work::places_read()), (0, 0), "nothing is read where no node is held");
}

/// An undo puts every node back where it stood, in what a model writer makes.
#[test]
fn an_undo_puts_each_node_back_where_it_stood() {
    let mut failures = Vec::new();
    let cases = cases();
    for case in &cases {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &(case.package)());
        let native = dir.path("a.diacraft");
        save_as(&mut open(&vsdx), &native);
        let mut s = open(&native);
        (case.run)(&mut s);
        for _ in 0..case.steps {
            s.execute("edit.undo", &json!({})).unwrap_or_else(|e| panic!("{}: the undo: {e}", case.name));
        }
        let out = dir.path("undone.vsdx");
        save_as(&mut s, &out);
        check(case, &before(case), &dir.read("undone.vsdx"), "after the undo", &mut failures);
    }
    report(failures, cases.len());
}

// ------------------------------------------------------------------------------------------------------------------------- a save as an .odg

/// A shape of a VSDX drawing saved as an `.odg` and that file saved as a VSDX file: the copy of the shape the `.odg` save is given holds the frame and the style as constants, the
/// geometry the master gives, and the sections in another order (`odgfile/flatten.rs`), and **each unknown node stays after the cell or the section it followed**.
struct Trip {
    name: &'static str,
    package: fn() -> Vec<u8>,
    /// The `ID` the shape has in the file that comes back (an `.odg` numbers a group again).
    shape: &'static str,
    expect: Vec<String>,
}

fn trips() -> Vec<Trip> {
    let mut list = Vec::new();
    let mut trip = |name: &'static str, package: fn() -> Vec<u8>, shape: &'static str, expect: &[&str]| {
        list.push(Trip { name, package, shape, expect: words(expect) });
    };
    trip(
        "a shape with its own cells and sections: the geometry is made first, and the node between the sections stays after the section it followed",
        change_package,
        "*3",
        &[
            "c:Height",
            "#after Height",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterCells",
            "Geometry0[r1,r2]",
            "Scratch[?Lead,r0,?Hint,r1,?Trail]",
            "#between the sections",
            "Hyperlink[?H0,rRow_1,?H1]",
            "?AfterSections",
            "Text",
        ],
    );
    trip(
        "an instance: the frame cells the copy holds go in among the cells the instance held, ahead of the nodes that followed the cell before them",
        instance_package,
        "*4",
        &[
            "?Lead",
            "c:Height",
            "c:LocPinX",
            "c:LocPinY",
            "c:PinX",
            "?AfterPinX",
            "c:PinY",
            "c:Width",
            "?AfterPinY",
            "Geometry0[r1,r2]",
            "Scratch[?SLead,r0,?STrail]",
            "?AfterSections",
        ],
    );
    trip(
        "an instance whose master gives its text a row: the row goes in among the section's own, ahead of the node before the first of them, and the nodes stay after the rows they followed",
        text_package,
        "*4",
        &["c:Height", "c:PinX", "c:PinY", "c:Width", "Character[?CLead,r0,r1,?CMid,r2,?CTrail]", "Text"],
    );
    trip(
        "a guide: the two pattern cells the copy gives it go in ahead of the nodes that followed the cell before them, and the node before the first cell stays there",
        guide_package,
        "*9",
        &["?Lead", "c:FillPattern", "c:LinePattern", "c:PinX", "?AfterPinX", "c:PinY", "?AfterPinY"],
    );
    trip(
        "a group: its frame stays what it was and the nodes stay where they stood among its cells, its sections and its members",
        group_package,
        "*1",
        &[
            "?Lead",
            "c:Height",
            "?AfterHeight",
            "c:LocPinX",
            "c:LocPinY",
            "c:PinX",
            "c:PinY",
            "?AfterPinY",
            "c:Width",
            "User[?ULead,rNote,?UTrail]",
            "?AfterSections",
            "?Shapes",
        ],
    );
    list
}

#[test]
fn a_save_as_an_odg_keeps_each_unknown_node_after_the_cell_or_the_section_it_followed_in_the_file_saved_as_vsdx_again() {
    let mut failures = Vec::new();
    let trips = trips();
    for trip in &trips {
        let dir = Dir::new();
        let vsdx = dir.write("a.vsdx", &(trip.package)());
        let mut s = open(&vsdx);
        let odg = dir.path("b.odg");
        save_as(&mut s, &odg);
        let mut again = open(&odg);
        let out = dir.path("c.vsdx");
        save_as(&mut again, &out);
        let seen = outline(&dir.read("c.vsdx"), trip.shape);
        if seen != trip.expect {
            failures.push(format!("{}:\n      want {:?}\n      got  {seen:?}", trip.name, trip.expect));
        }
    }
    report(failures, trips.len());
}
