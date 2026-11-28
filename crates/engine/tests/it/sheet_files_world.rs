//! What `sheet_files.rs` is made of: the drawing every trip starts from, written by hand from the specs' XML,
//! and the comparisons a trip makes: of two packages part by part, of two parts element by element, and of two models sheet by sheet. No file made by another
//! program is used.
//!
//! **The drawing** holds what a sheet edit can reach: a master with a path, connection points, scratch rows, named rows and shape data; an instance of it that overrides a cell and
//! inherits the rest; a shape of its own with two paths, every section a row can be added to and formulas that name rows of each other; a connector glued by `Connect` rows to a
//! connection point of the instance and one of the shape; a group with a member that reads the document's sheet; a second page with an instance; a style the shape's line cells
//! follow; the page's and the document's sheet. **Every cached value is the one its formula comes to**, so that a recalculation of a whole page, which a style's or the document's edit
//! makes, writes only the cells the edit reaches. Unknown attributes, unknown elements, comments and the white space of a file written by hand sit on the elements an edit touches and
//! on those it does not, and a trip fails when any of them is lost.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{Cells, Document, Passthrough, Placed, Row, Section, Shape, ShapeList, Sheet};
use diacraft_opc::xml::{El, parse};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, NS_2012, NS_R, Namespace, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::Variant;
use serde_json::{Value, json};

/// The master's shape: a box two by one, with the sections a sheet edit can reach.
pub const MASTER_SHAPE: &str = r#"
<Shape ID="5" Type="Shape" NameU="Box" Name="Box">
  <Cell N="PinX" V="1" F="Width*0.5"/>
  <Cell N="PinY" V="0.5" F="Height*0.5"/>
  <Cell N="Width" V="2"/>
  <Cell N="Height" V="1"/>
  <Cell N="LocPinX" V="1" F="Width*0.5"/>
  <Cell N="LocPinY" V="0.5" F="Height*0.5"/>
  <Section N="Geometry" IX="0">
    <Cell N="NoFill" V="0"/><Cell N="NoLine" V="0"/><Cell N="NoShow" V="0"/><Cell N="NoSnap" V="0"/><Cell N="NoQuickDrag" V="0"/>
    <Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
    <Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
    <Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
  </Section>
  <Section N="Connection">
    <Row IX="0"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/><Cell N="Type" V="0"/></Row>
    <Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/><Cell N="Type" V="0"/></Row>
  </Section>
  <Section N="Scratch">
    <Row IX="0"><Cell N="A" V="10"/><Cell N="B" V="20"/></Row>
    <Row IX="1"><Cell N="A" V="30" F="Scratch.A1*3"/><Cell N="B" V="40"/></Row>
  </Section>
  <Section N="User">
    <Row N="Rate"><Cell N="Value" V="5"/><Cell N="Prompt" V="Rate per box"/></Row>
    <Row N="Total"><Cell N="Value" V="10" F="Width*User.Rate"/></Row>
  </Section>
  <Section N="Property">
    <Row N="Cost">
      <Cell N="Label" V="Cost"/><Cell N="Prompt" V=""/><Cell N="Type" V="2"/><Cell N="Format" V=""/><Cell N="SortKey" V=""/><Cell N="Invisible" V="0"/>
      <Cell N="Verify" V="0"/><Cell N="DataLinked" V="0"/><Cell N="LangID" V="1033"/><Cell N="Calendar" V="0"/><Cell N="Value" V="12.5"/>
    </Row>
  </Section>
</Shape>"#;

/// An instance of the master that overrides one cell of a named row, holds two cells as the master's formulas (`Inh`) and inherits the rest.
const INSTANCE_10: &str = r#"
<Shape ID="10" Type="Shape" Master="1" NameU="Box.10" Name="Box.10">
  <Cell N="PinX" V="2"/>
  <Cell N="PinY" V="3"/>
  <Cell N="LocPinX" V="1" F="Inh"/>
  <Section N="User" IX="0">
    <Row N="Rate"><Cell N="Value" V="8"/></Row>
    <Row N="Total"><Cell N="Value" V="16" F="Inh"/></Row>
  </Section>
</Shape>"#;

/// A shape of its own: two paths, every section a row can be added to, formulas that name rows of each other, the style's line cell, and what a file written by hand carries that no
/// field of the model reads.
const SHAPE_11: &str = r#"
<Shape ID="11" Type="Shape" NameU="Own" Name="Own" LineStyle="3" FillStyle="0" TextStyle="0" Foo="kept">
  <!-- a comment between the cells of a shape -->
  <Cell N="PinX" V="6"/>
  <Cell N="PinY" V="3"/>
  <Cell N="Width" V="3" F="Height*3" Bar="kept"/>
  <Cell N="Height" V="1"/>
  <Cell N="LocPinX" V="1.5" F="Width*0.5"/>
  <Cell N="LocPinY" V="0.5" F="Height*0.5"/>
  <Cell N="LineWeight" V="0.01" F="Inh"/>
  <Section N="Geometry" IX="0" Baz="kept">
    <Cell N="NoFill" V="0"/><Cell N="NoLine" V="0"/><Cell N="NoShow" V="0"/><Cell N="NoSnap" V="0"/><Cell N="NoQuickDrag" V="0"/>
    <Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="2"><Cell N="X" V="3" F="Width"/><Cell N="Y" V="0"/></Row>
    <x:Hint xmlns:x="urn:test:unknown" at="between the rows"/>
    <Row T="LineTo" IX="3" Qux="kept"><Cell N="X" V="3" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
    <Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
    <Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
  </Section>
  <Section N="Geometry" IX="1">
    <Cell N="NoFill" V="1"/><Cell N="NoLine" V="0"/><Cell N="NoShow" V="0"/><Cell N="NoSnap" V="0"/><Cell N="NoQuickDrag" V="0"/>
    <Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="2"><Cell N="X" V="1.5" F="Width*0.5"/><Cell N="Y" V="0.5" F="Height*0.5"/></Row>
  </Section>
  <Section N="Connection" IX="0">
    <Row IX="0"><Cell N="X" V="3" F="Width"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/><Cell N="Type" V="0"/></Row>
    <Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/><Cell N="Type" V="0"/></Row>
    <Row IX="2"><Cell N="X" V="1.5" F="Width*0.5"/><Cell N="Y" V="1" F="Height"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/><Cell N="Type" V="0"/></Row>
  </Section>
  <Section N="Scratch">
    <Row IX="0"><Cell N="A" V="3" F="Geometry1.X3"/><Cell N="B" V="0"/></Row>
    <Row IX="1"><Cell N="A" V="4" F="Scratch.A1+1"/><Cell N="B" V="1"/></Row>
    <Row IX="2" Zip="kept"><Cell N="A" V="7"/><Cell N="B" V="2"/><x:Mark xmlns:x="urn:test:unknown" at="row"/></Row>
  </Section>
  <Section N="User">
    <Row N="Rate"><Cell N="Value" V="5"/><Cell N="Prompt" V=""/></Row>
    <Row N="Total"><Cell N="Value" V="15" F="Width*User.Rate"/></Row>
    <Row N="Mirror"><Cell N="Value" V="7" F="Scratch.A3"/></Row>
    <!-- a comment between the rows of a named section -->
    <Row N="Größe"><Cell N="Value" V="2"/></Row>
  </Section>
  <Section N="Property">
    <Row N="Cost">
      <Cell N="Label" V="Cost"/><Cell N="Prompt" V=""/><Cell N="Type" V="2"/><Cell N="Format" V=""/><Cell N="SortKey" V=""/><Cell N="Invisible" V="0"/>
      <Cell N="Verify" V="0"/><Cell N="DataLinked" V="0"/><Cell N="LangID" V="1033"/><Cell N="Calendar" V="0"/><Cell N="Value" V="12.5"/>
    </Row>
    <Row N="Owner" LocalName="Besitzer">
      <Cell N="Label" V="Owner"/><Cell N="Prompt" V=""/><Cell N="Type" V="0"/><Cell N="Format" V=""/><Cell N="SortKey" V=""/><Cell N="Invisible" V="0"/>
      <Cell N="Verify" V="0"/><Cell N="DataLinked" V="0"/><Cell N="LangID" V="1033"/><Cell N="Calendar" V="0"/><Cell N="Value" V="Ann"/>
    </Row>
  </Section>
  <Section N="Hyperlink">
    <Row N="Row_1"><Cell N="Description" V="Site"/><Cell N="Address" V="https://example.org/"/><Cell N="SubAddress" V=""/><Cell N="Frame" V=""/></Row>
  </Section>
  <Section N="Control">
    <Row N="Row_1"><Cell N="X" V="1.5" F="Width*0.5"/><Cell N="Y" V="0" F="Height*0"/><Cell N="XDyn" V="1.5" F="Width*0.5"/><Cell N="YDyn" V="0" F="Height*0"/><Cell N="CanGlue" V="0"/></Row>
  </Section>
  <Section N="Actions">
    <Row N="Row_1"><Cell N="Menu" V="Open"/><Cell N="Action" V="0"/><Cell N="Disabled" V="0"/></Row>
  </Section>
  <x:Unknown xmlns:x="urn:test:unknown" kept="yes"/>
</Shape>"#;

/// A connector glued to a connection point of shape 10 and one of shape 11, by the formulas and the `Connect` rows a file holds.
const CONNECTOR_12: &str = r#"
<Shape ID="12" Type="Shape" NameU="Dynamic connector" Name="Dynamic connector">
  <Cell N="BeginX" V="3" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/>
  <Cell N="BeginY" V="3" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/>
  <Cell N="EndX" V="4.5" F="PAR(PNT(Sheet.11!Connections.X2,Sheet.11!Connections.Y2))"/>
  <Cell N="EndY" V="3" F="PAR(PNT(Sheet.11!Connections.X2,Sheet.11!Connections.Y2))"/>
  <Cell N="PinX" V="3.75" F="(BeginX+EndX)/2"/>
  <Cell N="PinY" V="3" F="(BeginY+EndY)/2"/>
  <Cell N="Width" V="1.5" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/>
  <Cell N="Height" V="0"/>
  <Cell N="LocPinX" V="0.75" F="Width*0.5"/>
  <Cell N="LocPinY" V="0" F="Height*0.5"/>
  <Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
  <Section N="Geometry" IX="0">
    <Cell N="NoFill" V="1"/>
    <Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="2"><Cell N="X" V="1.5" F="Width"/><Cell N="Y" V="0" F="Height"/></Row>
  </Section>
</Shape>"#;

/// A group of one member, which reads the document's sheet.
const GROUP_13: &str = r#"
<Shape ID="13" Type="Group" NameU="Group" Name="Group">
  <Cell N="PinX" V="9"/>
  <Cell N="PinY" V="3"/>
  <Cell N="Width" V="2"/>
  <Cell N="Height" V="2"/>
  <Cell N="LocPinX" V="1" F="Width*0.5"/>
  <Cell N="LocPinY" V="1" F="Height*0.5"/>
  <Shapes>
    <Shape ID="14" Type="Shape" NameU="Member" Name="Member">
      <Cell N="PinX" V="1"/>
      <Cell N="PinY" V="1"/>
      <Cell N="Width" V="1" F="GUARD(1)"/>
      <Cell N="Height" V="1"/>
      <Cell N="LocPinX" V="0.5" F="Width*0.5"/>
      <Cell N="LocPinY" V="0.5" F="Height*0.5"/>
      <Section N="Geometry" IX="0">
        <Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
        <Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="0"/></Row>
        <Row T="LineTo" IX="3"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
      </Section>
      <Section N="Scratch">
        <Row IX="0"><Cell N="A" V="1"/></Row>
        <Row IX="2"><Cell N="A" V="2"/></Row>
        <Row IX="5"><Cell N="A" V="3"/></Row>
      </Section>
      <Section N="User">
        <Row N="Fee"><Cell N="Value" V="7" F="TheDoc!User.Rate+2"/></Row>
      </Section>
    </Shape>
  </Shapes>
</Shape>"#;

/// The connection points the connector is glued to, as a file writes them (`ToPart` is 100 and the row's `IX`; `X1` is the row `IX` 0).
const CONNECTS_1: &str = r#"
<Connect FromSheet="12" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Connections.X1" ToPart="100"/>
<Connect FromSheet="12" FromCell="EndX" FromPart="12" ToSheet="11" ToCell="Connections.X2" ToPart="101"/>
"#;

/// A shape whose Geometry sections have no `IX` (the reader numbers them by their place) and whose Scratch rows have none either.
const SHAPE_15: &str = r#"
<Shape ID="15" Type="Shape" NameU="Plain" Name="Plain">
  <Cell N="PinX" V="9"/>
  <Cell N="PinY" V="6"/>
  <Cell N="Width" V="1"/>
  <Cell N="Height" V="1"/>
  <Cell N="LocPinX" V="0.5" F="Width*0.5"/>
  <Cell N="LocPinY" V="0.5" F="Height*0.5"/>
  <Section N="Geometry">
    <Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="3"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
  </Section>
  <Section N="Geometry">
    <Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
    <Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="0"/></Row>
  </Section>
  <Section N="Scratch">
    <Row><Cell N="A" V="1"/></Row>
    <Row><Cell N="A" V="2" F="Scratch.A1+1"/></Row>
    <Row><Cell N="A" V="3" F="Scratch.A2+1"/></Row>
  </Section>
</Shape>"#;

const INSTANCE_40: &str = r#"
<Shape ID="40" Type="Shape" Master="1" NameU="Box.40" Name="Box.40">
  <Cell N="PinX" V="3"/>
  <Cell N="PinY" V="3"/>
  <Section N="Geometry" IX="0">
    <Row IX="3"><Cell N="X" V="2.5"/></Row>
  </Section>
  <Section N="Scratch">
    <Row IX="1"><Cell N="B" V="41"/></Row>
  </Section>
</Shape>"#;

const PAGE_1_SHEET: &str = r#"
  <Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/><Cell N="PageScale" V="1" U="IN"/><Cell N="DrawingScale" V="1" U="IN"/>
  <Section N="User"><Row N="Rate"><Cell N="Value" V="4"/></Row></Section>"#;

const MASTER_SHEET: &str =
    r#"<Cell N="PageWidth" V="2"/><Cell N="PageHeight" V="1"/><Section N="User"><Row N="Rate"><Cell N="Value" V="1"/></Row></Section>"#;

const DOCUMENT_SHEET: &str = r#"
  <Cell N="DocLangID" V="1033"/>
  <Section N="User"><Row N="Rate"><Cell N="Value" V="5"/></Row></Section>"#;

/// The style the shape's line follows: one cell of its own, a named row, and an unknown attribute.
const STYLE_3: &str = r#"
<StyleSheet ID="3" NameU="Plain" Name="Plain" LineStyle="0" FillStyle="0" TextStyle="0" Mark="kept">
  <Cell N="LineWeight" V="0.01"/>
  <Cell N="LineColor" V="1"/>
  <Section N="User"><Row N="Rate"><Cell N="Value" V="3"/></Row></Section>
</StyleSheet>"#;

/// The six types of the format, by the extension each is saved under.
pub const EXTENSIONS: [&str; 6] = ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"];

/// The part a macro-enabled type holds: the project, as bytes, and the relationship from the document part that names it.
fn macro_project() -> Pkg {
    const PROJECT: &str = "/visio/vbaProject.bin"; // brand-ok: a part name the format defines
    const DOCUMENT_PART: &str = "/visio/document.xml"; // brand-ok: a part name the format defines
    Pkg::new().part(PROJECT, "application/vnd.ms-office.vbaProject", b"not a project, the bytes a save must keep").rel(
        DOCUMENT_PART,
        "rId90",
        "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
        "vbaProject.bin",
    )
}

/// The package of the drawing as the type named by `extension` (`vsdx`, `vsdm`, `vssx`, `vssm`, `vstx`, `vstm`), with a part nothing models, and the macro project of a type that allows
/// macros.
pub fn package(extension: &str) -> Vec<u8> {
    package_in(extension, Namespace::Main2012)
}

/// [`package`] with every part of the format in the namespace `ns` (the one every sample uses, the one the standard states, or none at all).
pub fn package_in(extension: &str, ns: Namespace) -> Vec<u8> {
    let variant = Variant::from_extension(extension).unwrap_or_else(|| panic!("{extension} is no type of the format"));
    let mut master = MasterSpec::new(1, "Box").shapes(MASTER_SHAPE);
    master.sheet = Some(MASTER_SHEET.to_owned());
    let page_1 = format!("{INSTANCE_10}{SHAPE_11}{CONNECTOR_12}{GROUP_13}{SHAPE_15}");
    let mut builder = VsdxBuilder::new()
        .namespace(ns)
        .document_type(variant.content_type())
        .more_styles(STYLE_3)
        .document_sheet(DOCUMENT_SHEET)
        .master(master)
        .page(PageSpec::new(1, "Page-1").shapes(&page_1).connects(CONNECTS_1).sheet(PAGE_1_SHEET))
        .page(PageSpec::new(2, "Page-2").shapes(INSTANCE_40))
        .with(Pkg::new().part_without_override("/foo/bar.dat", b"0123456789").default_type("dat", "application/x-foo"));
    if variant.macro_enabled {
        builder = builder.with(macro_project());
    }
    builder.build()
}

/// `xml` with every element of the format's vocabulary written with the prefix `v:` (the elements of other vocabularies, which have a prefix already, are left as they are): the shapes of a
/// file whose writer declares no default namespace.
fn prefixed(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() + xml.len() / 8);
    let mut rest = xml;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let tag_end = rest.find('>').map_or(rest.len(), |e| e + 1);
        let (tag, tail) = rest.split_at(tag_end);
        let (open, name_at) = if tag.starts_with("</") { ("</", 2) } else { ("<", 1) };
        let name: String = tag[name_at..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let plain = !tag.starts_with("<!--") && !tag.starts_with("<?") && !name.is_empty() && !tag[name_at..].starts_with(&format!("{name}:"));
        if plain {
            out.push_str(&format!("{open}v:{}", &tag[name_at..]));
        } else {
            out.push_str(tag);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// The same drawing as [`package`], with the shapes of the page part and of the master part written under the prefix `v:` and no default namespace (the pages index, the masters index and the
/// document part are the builder's own).
pub fn prefixed_package(extension: &str) -> Vec<u8> {
    let variant = Variant::from_extension(extension).unwrap_or_else(|| panic!("{extension} is no type of the format"));
    let root = |name: &str, inner: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<v:{name} xmlns:v=\"{NS_2012}\" xmlns:r=\"{NS_R}\" xml:space=\"preserve\">{inner}</v:{name}>"
        )
    };
    let page_1 = format!("{INSTANCE_10}{SHAPE_11}{CONNECTOR_12}{GROUP_13}{SHAPE_15}");
    let page_1_part = root("PageContents", &prefixed(&format!("<Shapes>{page_1}</Shapes><Connects>{CONNECTS_1}</Connects>")));
    let page_2_part = root("PageContents", &prefixed(&format!("<Shapes>{INSTANCE_40}</Shapes>")));
    let master_part = root("MasterContents", &prefixed(&format!("<Shapes>{MASTER_SHAPE}</Shapes>")));
    let mut master = MasterSpec::new(1, "Box").part(&master_part);
    master.sheet = Some(MASTER_SHEET.to_owned());
    let mut builder = VsdxBuilder::new()
        .document_type(variant.content_type())
        .more_styles(STYLE_3)
        .document_sheet(DOCUMENT_SHEET)
        .master(master)
        .page(PageSpec::new(1, "Page-1").part(&page_1_part).sheet(PAGE_1_SHEET))
        .page(PageSpec::new(2, "Page-2").part(&page_2_part));
    if variant.macro_enabled {
        builder = builder.with(macro_project());
    }
    builder.build()
}

/// A stencil as a person's tool makes one: masters and no page, in the type named by `extension`.
pub fn stencil_package(extension: &str) -> Vec<u8> {
    let variant = Variant::from_extension(extension).unwrap_or_else(|| panic!("{extension} is no type of the format"));
    let mut master = MasterSpec::new(1, "Box").shapes(MASTER_SHAPE);
    master.sheet = Some(MASTER_SHEET.to_owned());
    let mut builder = VsdxBuilder::new()
        .document_type(variant.content_type())
        .more_styles(STYLE_3)
        .document_sheet(DOCUMENT_SHEET)
        .master(master)
        .without_pages_part();
    if variant.macro_enabled {
        builder = builder.with(macro_project());
    }
    builder.build()
}

// ---------------------------------------------------------------------------------------------------------------------------- packages

/// The parts of a package by name, each with its bytes.
pub fn parts_of(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    package.parts().map(|name| (name.to_owned(), package.part(name).unwrap().to_vec())).collect()
}

/// The names of the parts whose bytes differ between two packages, and those one holds and the other does not.
pub fn changed_parts(before: &[u8], after: &[u8]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (a, b) = (parts_of(before), parts_of(after));
    let changed = a.iter().filter(|(n, bytes)| b.get(*n).is_some_and(|other| other != *bytes)).map(|(n, _)| n.clone()).collect();
    let removed = a.keys().filter(|n| !b.contains_key(*n)).cloned().collect();
    let added = b.keys().filter(|n| !a.contains_key(*n)).cloned().collect();
    (changed, removed, added)
}

// ------------------------------------------------------------------------------------------------------------------------------ parts

/// The identity of an element for the path of a record: what tells it from its siblings.
fn segment(el: &El) -> String {
    let attrs: &[&str] = match el.local() {
        "Shape" | "Page" | "Master" | "StyleSheet" => &["ID"],
        "Section" | "Row" => &["N", "IX"],
        "Cell" => &["N"],
        "Connect" => &["FromSheet", "FromCell", "ToSheet", "ToCell"],
        _ => &[],
    };
    let ids: Vec<String> = attrs.iter().filter_map(|a| el.attr(a).map(|v| format!("{a}={v}"))).collect();
    if ids.is_empty() { el.local().to_owned() } else { format!("{}[{}]", el.local(), ids.join("|")) }
}

fn walk(el: &El, path: &str, out: &mut Vec<(String, String)>) {
    let here = if path.is_empty() { segment(el) } else { format!("{path}/{}", segment(el)) };
    let mut attrs: Vec<String> = el.attrs.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
    let text = el.text();
    if !text.trim().is_empty() {
        attrs.push(format!("#text={:?}", text.trim()));
    }
    out.push((here.clone(), attrs.join(" ")));
    for child in el.elements() {
        walk(child, &here, out);
    }
}

/// Every element of a part as `(path, its own attributes and text)`, in document order. A path names each element by what tells it from its siblings.
pub fn records(part: &[u8]) -> Vec<Record> {
    let root = parse(part).unwrap_or_else(|e| panic!("a part that is not XML: {e}"));
    let mut out = Vec::new();
    walk(&root, "", &mut out);
    out
}

/// Whether `path` is under one of `allowed`: an entry that ends with `/` allows what is below it and not the element itself, any other the element and what is below it.
fn allowed(path: &str, allowed: &[&str]) -> bool {
    allowed.iter().any(|a| if a.ends_with('/') { path.starts_with(a) } else { path == *a || path.starts_with(&format!("{a}/")) })
}

/// An element of a part as [`records`] gives it: its path and its own attributes and text.
pub type Record = (String, String);

/// What differs between two versions of a part, as records: those only the first has, and those only the second has, in the order of their parts.
pub fn differing_records(before: &[u8], after: &[u8]) -> (Vec<Record>, Vec<Record>) {
    let (a, b) = (records(before), records(after));
    let mut count: BTreeMap<&Record, isize> = BTreeMap::new();
    for r in &a {
        *count.entry(r).or_default() += 1;
    }
    for r in &b {
        *count.entry(r).or_default() -= 1;
    }
    let only_before = a.iter().filter(|r| count.get(r).copied().unwrap_or(0) > 0).cloned().collect();
    let only_after = b.iter().filter(|r| count.get(r).copied().unwrap_or(0) < 0).cloned().collect();
    (only_before, only_after)
}

/// Checks that two versions of a part differ only inside `regions` (see [`allowed`]): every element that differs is in one, and **the elements outside them are the same, with the same
/// attributes, in the same order**. → why not.
pub fn only_within(before: &[u8], after: &[u8], regions: &[&str]) -> Result<(), String> {
    let (gone, new) = differing_records(before, after);
    let outside: Vec<String> = gone
        .iter()
        .map(|(path, attrs)| (path, attrs, "was"))
        .chain(new.iter().map(|(path, attrs)| (path, attrs, "is")))
        .filter(|(path, _, _)| !allowed(path, regions))
        .map(|(path, attrs, word)| format!("{word} {path} {attrs}"))
        .collect();
    if !outside.is_empty() {
        let shown: Vec<&String> = outside.iter().take(6).collect();
        return Err(format!(
            "{} elements outside the edit differ:\n        {}",
            outside.len(),
            shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n        ")
        ));
    }
    let outside = |list: Vec<(String, String)>| -> Vec<(String, String)> { list.into_iter().filter(|(p, _)| !allowed(p, regions)).collect() };
    let (a, b) = (outside(records(before)), outside(records(after)));
    if a != b {
        let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        return Err(format!("the elements outside the edit are not in the same order: first at {at}: {:?} and {:?}", a.get(at), b.get(at)));
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------------------------------------ models

/// The same nodes, each saying it stands after none of the typed children before it. **A comparison that is not exact leaves the place of an unknown node out**: a save into a VSDX
/// file splices and keeps each node attached to the element that held the item before it, and the file read back counts the typed children before it again, so two models of the same
/// file can differ in the counts and in nothing else; the nodes and their order are read.
fn unplaced(p: &Passthrough) -> Passthrough {
    Passthrough::from_nodes(p.iter().map(|placed| Placed { after: 0, node: placed.node.clone() }).collect())
}

/// A sheet with **the `IX` that a section other than `Geometry` holds among its unknown attributes taken off** (no such section of a file has one, so a
/// section that a save writes anew has none, and the model keeps the attribute a file wrote until the file is read again).
fn without_ix(sheet: &Sheet) -> Sheet {
    let mut s = sheet.clone();
    for section in s.sections.iter_mut() {
        if section.name.as_str() != "Geometry" && section.raw.get("IX").is_some() {
            let section = Arc::make_mut(section);
            section.raw = diacraft_doc::RawAttrs::from_pairs(section.raw.iter().filter(|(name, _)| *name != "IX"));
        }
    }
    s
}

/// [`without_ix`], and the counts of every unknown node in the sheet (its own, its sections' and its rows') put to 0 (see [`unplaced`]).
fn without_places(sheet: &Sheet) -> Sheet {
    let mut s = without_ix(sheet);
    s.passthrough = unplaced(&s.passthrough);
    for section in s.sections.iter_mut() {
        let section = Arc::make_mut(section);
        section.passthrough = unplaced(&section.passthrough);
        for row in section.rows.iter_mut() {
            let row = Arc::make_mut(row);
            row.passthrough = unplaced(&row.passthrough);
        }
    }
    s
}

/// Every sheet of a document by a name that says where it is: `document`, `style 3`, `master 1`, `master 1 shape 5`, `page 1`, `page 1 shape 11` and `page 1 shape 13/14`.
pub fn sheets(doc: &Document) -> BTreeMap<String, Arc<Sheet>> {
    let mut out = BTreeMap::new();
    out.insert("document".to_owned(), Arc::clone(&doc.sheet));
    for style in doc.styles.iter() {
        out.insert(format!("style {}", style.id), Arc::clone(&style.sheet));
    }
    for master in doc.masters.iter() {
        out.insert(format!("master {}", master.id), Arc::clone(&master.sheet));
        shapes(&format!("master {} shape ", master.id), &master.shapes, &mut out);
    }
    for page in doc.pages.iter() {
        out.insert(format!("page {}", page.id), Arc::clone(&page.sheet));
        shapes(&format!("page {} shape ", page.id), &page.shapes, &mut out);
    }
    out
}

fn shapes(prefix: &str, list: &ShapeList, out: &mut BTreeMap<String, Arc<Sheet>>) {
    for shape in list.iter() {
        member(prefix, shape, out);
    }
}

fn member(prefix: &str, shape: &Shape, out: &mut BTreeMap<String, Arc<Sheet>>) {
    out.insert(format!("{prefix}{}", shape.id), Arc::clone(&shape.sheet));
    for child in shape.children.iter() {
        member(&format!("{prefix}{}/", shape.id), child, out);
    }
}

fn cells_differ(what: &str, a: &Cells, b: &Cells, out: &mut Vec<String>) {
    for (name, cell) in a {
        match b.get(name) {
            None => out.push(format!("{what}: {} is gone", name.as_str())),
            Some(other) if other != cell => out.push(format!("{what}: {} was {cell:?} and is {other:?}", name.as_str())),
            Some(_) => {}
        }
    }
    for name in b.keys().filter(|n| !a.contains_key(*n)) {
        out.push(format!("{what}: {} is new", name.as_str()));
    }
}

fn row_differs(what: &str, a: &Row, b: &Row, out: &mut Vec<String>) {
    if (a.name.as_deref(), a.ix, a.del, a.row_type.as_deref(), a.local_name.as_deref())
        != (b.name.as_deref(), b.ix, b.del, b.row_type.as_deref(), b.local_name.as_deref())
    {
        out.push(format!(
            "{what}: the row was N={:?} IX={:?} Del={} T={:?} and is N={:?} IX={:?} Del={} T={:?}",
            a.name, a.ix, a.del, a.row_type, b.name, b.ix, b.del, b.row_type
        ));
    }
    if a.raw != b.raw || a.passthrough != b.passthrough || a.ext != b.ext {
        out.push(format!("{what}: what the row holds besides its cells differs"));
    }
    cells_differ(what, &a.cells, &b.cells, out);
}

fn section_differs(what: &str, a: &Section, b: &Section, out: &mut Vec<String>) {
    if (a.del, a.ix) != (b.del, b.ix) {
        out.push(format!("{what}: the section was Del={} IX={:?} and is Del={} IX={:?}", a.del, a.ix, b.del, b.ix));
    }
    if a.raw != b.raw || a.passthrough != b.passthrough || a.ext != b.ext {
        out.push(format!("{what}: what the section holds besides its cells and rows differs"));
    }
    cells_differ(what, &a.cells, &b.cells, out);
    if a.rows.len() != b.rows.len() {
        out.push(format!("{what}: the section had {} rows and has {}", a.rows.len(), b.rows.len()));
    }
    for (k, (x, y)) in a.rows.iter().zip(b.rows.iter()).enumerate() {
        if x != y {
            row_differs(&format!("{what}, row {k}"), x, y, out);
        }
    }
}

/// How two sheets differ, in words that name the cell, the row and the section: empty when they are equal.
pub fn sheet_differs(a: &Sheet, b: &Sheet) -> Vec<String> {
    let mut out = Vec::new();
    if a == b {
        return out;
    }
    cells_differ("sheet", &a.cells, &b.cells, &mut out);
    if (a.links, &a.raw, &a.passthrough, &a.ext) != (b.links, &b.raw, &b.passthrough, &b.ext) {
        out.push("sheet: its links, attributes or unknown elements differ".to_owned());
    }
    if a.sections.len() != b.sections.len() {
        out.push(format!("sheet: {} sections and now {}", a.sections.len(), b.sections.len()));
    }
    for (k, (x, y)) in a.sections.iter().zip(b.sections.iter()).enumerate() {
        if x.name != y.name {
            out.push(format!("section {k} was {} and is {}", x.name.as_str(), y.name.as_str()));
        } else if x != y {
            section_differs(&format!("{} IX={:?}", x.name.as_str(), x.ix), x, y, &mut out);
        }
    }
    if out.is_empty() {
        out.push("the sheets differ in something this walk does not name".to_owned());
    }
    out
}

/// The shapes of a document by the name [`sheets`] gives their sheets, each as the model holds it but for its sheet and its members (which are shapes of their own).
fn bare_shapes(doc: &Document, exact: bool) -> BTreeMap<String, Shape> {
    fn walk(prefix: &str, shape: &Shape, exact: bool, out: &mut BTreeMap<String, Shape>) {
        let mut bare = shape.clone();
        bare.sheet = Arc::default();
        bare.children = ShapeList::default();
        if !exact {
            bare.passthrough = unplaced(&bare.passthrough);
        }
        out.insert(format!("{prefix}{}", shape.id), bare);
        for child in shape.children.iter() {
            walk(&format!("{prefix}{}/", shape.id), child, exact, out);
        }
    }
    let mut out = BTreeMap::new();
    for master in doc.masters.iter() {
        for shape in master.shapes.iter() {
            walk(&format!("master {} shape ", master.id), shape, exact, &mut out);
        }
    }
    for page in doc.pages.iter() {
        for shape in page.shapes.iter() {
            walk(&format!("page {} shape ", page.id), shape, exact, &mut out);
        }
    }
    out
}

/// What a shape holds besides its sheet and its members: the field that differs, by name.
fn shape_differs(a: &Shape, b: &Shape) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (word, same) in [
        (
            "identity",
            (a.id, &a.name, &a.name_u, &a.kind, a.master, a.master_shape, a.del)
                == (b.id, &b.name, &b.name_u, &b.kind, b.master, b.master_shape, b.del),
        ),
        ("text", a.text == b.text),
        ("data", (&a.data1, &a.data2, &a.data3) == (&b.data1, &b.data2, &b.data3)),
        ("picture", a.foreign == b.foreign),
        ("attributes", a.raw == b.raw),
        ("unknown elements", a.passthrough == b.passthrough),
        ("held data", a.ext == b.ext),
    ] {
        if !same {
            out.push(word);
        }
    }
    out
}

/// How two documents differ in any sheet (see [`sheets`]) or in what a shape holds besides its sheet, one line each, at most thirty. With `exact` false the place each unknown node says
/// it stands is not compared (see [`unplaced`]).
pub fn sheets_differ(a: &Document, b: &Document, exact: bool) -> Vec<String> {
    let (x, y) = (sheets(a), sheets(b));
    let mut out = Vec::new();
    let (bare_a, bare_b) = (bare_shapes(a, exact), bare_shapes(b, exact));
    for (name, shape) in &bare_a {
        if let Some(other) = bare_b.get(name) {
            out.extend(shape_differs(shape, other).into_iter().map(|what| format!("{name}: the shape's {what} differ")));
        }
    }
    // What a page, a master and a style hold besides their sheets and shapes (the relationship and part names are the package's, which a save numbers anew).
    for (p, q) in a.pages.iter().zip(b.pages.iter()) {
        let bare = |page: &diacraft_doc::Page| diacraft_doc::Page {
            sheet: Arc::default(),
            shapes: ShapeList::default(),
            rel_id: None,
            part_name: None,
            passthrough: if exact { page.passthrough.clone() } else { unplaced(&page.passthrough) },
            ..page.clone()
        };
        if bare(p) != bare(q) {
            out.push(format!("page {}: what the page holds besides its sheet and its shapes differs", p.id));
        }
    }
    for (p, q) in a.masters.iter().zip(b.masters.iter()) {
        let bare = |master: &diacraft_doc::Master| diacraft_doc::Master {
            sheet: Arc::default(),
            shapes: ShapeList::default(),
            rel_id: None,
            part_name: None,
            passthrough: if exact { master.passthrough.clone() } else { unplaced(&master.passthrough) },
            ..master.clone()
        };
        if bare(p) != bare(q) {
            out.push(format!("master {}: what the master holds besides its sheet and its shapes differs", p.id));
        }
    }
    for (p, q) in a.styles.iter().zip(b.styles.iter()) {
        let bare = |style: &diacraft_doc::Style| diacraft_doc::Style {
            sheet: Arc::default(),
            passthrough: if exact { style.passthrough.clone() } else { unplaced(&style.passthrough) },
            ..style.clone()
        };
        if bare(p) != bare(q) {
            out.push(format!("style {}: what the style holds besides its sheet differs", p.id));
        }
    }
    if a.pages.len() != b.pages.len() || a.masters.len() != b.masters.len() || a.styles.len() != b.styles.len() {
        out.push("the drawing has other pages, masters or styles".to_owned());
    }
    if a.passthrough != b.passthrough {
        out.push("the document's unknown elements differ".to_owned());
    }
    for (name, sheet) in &x {
        match y.get(name) {
            None => out.push(format!("{name}: the sheet is gone")),
            Some(other) if other != sheet => {
                let (p, q) = if exact { (without_ix(sheet), without_ix(other)) } else { (without_places(sheet), without_places(other)) };
                if p != q {
                    out.extend(sheet_differs(&p, &q).into_iter().map(|d| format!("{name}: {d}")));
                }
            }
            Some(_) => {}
        }
    }
    out.extend(y.keys().filter(|n| !x.contains_key(*n)).map(|n| format!("{n}: the sheet is new")));
    out.truncate(30);
    out
}

// -------------------------------------------------------------------------------------------------------------------------- the reads

/// Every line of a sheet as `sheet.read` gives it, with every section it knows (`all`), in pages of its own size: the sections and the lines, to compare.
pub fn read_all(session: &mut diacraft_engine::Session, target: &Value) -> Value {
    let mut lines: Vec<Value> = Vec::new();
    let mut sections;
    let mut from = 0u64;
    loop {
        let mut params = target.clone();
        let map = params.as_object_mut().unwrap();
        map.insert("all".into(), json!(true));
        map.insert("from".into(), json!(from));
        map.insert("count".into(), json!(500));
        let page = session.execute("sheet.read", &params).unwrap_or_else(|e| panic!("sheet.read {target}: {e}"));
        sections = page["sections"].clone();
        let got = page["lines"].as_array().cloned().unwrap_or_default();
        from += got.len() as u64;
        lines.extend(got);
        if page["more"] != json!(true) || lines.len() > 20_000 {
            break;
        }
    }
    json!({ "sections": sections, "lines": lines })
}
