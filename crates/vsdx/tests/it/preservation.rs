//! Preservation, ahead of M3: a read and a re-emit of each part holds the same text for every cell, row, section and shape the file had
//! The re-emitter is `emit.rs`, which builds a tree from the model alone: what
//! it cannot give back, the model has lost.

use diacraft_opc::xml::{self, El};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::package::{MasterSpec, OD, PageSpec, Pkg, VsdxBuilder};
use proptest::prelude::*;

use crate::emit::{connects, shape, shapes, sheet_only, signature, style};
use crate::read_ok;

fn tree_of(bytes: &[u8], part: &str) -> El {
    let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
    xml::parse(pkg.part(part).unwrap_or_else(|| panic!("no part {part}"))).unwrap()
}

/// Equal signatures, or a failure that shows where they first differ.
fn same(what: &str, left: &str, right: &str) {
    if left == right {
        return;
    }
    let at = left.chars().zip(right.chars()).take_while(|(a, b)| a == b).count();
    let from = at.saturating_sub(120);
    let window = |s: &str| s.chars().skip(from).take(300).collect::<String>();
    panic!("{what}: the signatures differ at character {at}\n   re-emitted: ...{}\n   original:   ...{}", window(left), window(right));
}

fn child<'a>(root: &'a El, local: &str) -> &'a El {
    root.child(local).unwrap_or_else(|| panic!("no child {local} in {}", root.name))
}

/// The page and master parts, the pages and masters indexes' sheets, the styles and the document sheet of a package, each against what the
/// model gives back.
fn check_round_trip(bytes: &[u8]) {
    let (doc, _) = read_ok(bytes);
    for (i, page) in doc.pages.iter().enumerate() {
        let root = tree_of(bytes, &format!("/visio/pages/page{}.xml", i + 1));
        if let Some(original) = root.child("Shapes") {
            same(&format!("page {} shapes", i + 1), &signature(&shapes(&page.shapes)), &signature(original));
        } else {
            assert!(page.shapes.is_empty());
        }
        if let Some(original) = root.child("Connects") {
            same(&format!("page {} connects", i + 1), &signature(&connects(&page.connects)), &signature(original));
        }
    }
    for (i, master) in doc.masters.iter().enumerate() {
        let root = tree_of(bytes, &format!("/visio/masters/master{}.xml", i + 1));
        if let Some(original) = root.child("Shapes") {
            same(&format!("master {} shapes", i + 1), &signature(&shapes(&master.shapes)), &signature(original));
        }
    }
    let pages_index = tree_of(bytes, "/visio/pages/pages.xml");
    for (page, row) in doc.pages.iter().zip(pages_index.children_named("Page")) {
        if let Some(original) = row.child("PageSheet") {
            same("a PageSheet", &signature(&sheet_only("PageSheet", &page.sheet)), &signature(original));
        }
    }
    let masters_index = tree_of(bytes, "/visio/masters/masters.xml");
    for (master, row) in doc.masters.iter().zip(masters_index.children_named("Master")) {
        if let Some(original) = row.child("PageSheet") {
            same("a master's PageSheet", &signature(&sheet_only("PageSheet", &master.sheet)), &signature(original));
        }
    }
    let document = tree_of(bytes, "/visio/document.xml");
    let original_styles: Vec<&El> = child(&document, "StyleSheets").children_named("StyleSheet").collect();
    assert_eq!(original_styles.len(), doc.styles.len());
    for (st, original) in doc.styles.iter().zip(original_styles) {
        same(&format!("style {}", st.id), &signature(&style(st)), &signature(original));
    }
    same("the DocumentSheet", &signature(&sheet_only("DocumentSheet", &doc.sheet)), &signature(child(&document, "DocumentSheet")));
}

pub(crate) const RICH_SHAPES: &str = r##"
<Shape ID="1" Name="Group &amp; co" NameU="g" Type="Group" Master="1" LineStyle="1" FillStyle="" TextStyle="007" OriginalID="9" UniqueID="{AA}" x:mine="1" xmlns:x="urn:example">
  <Cell N="Angle" V="0.5" U="DEG"/>
  <Cell N="FlipX" V="0" F="Inh"/>
  <Cell N="Height" V="1.50" F="Width*0.75" U="MM"/>
  <Cell N="LineColor" V="Themed" F="THEMEVAL()"/>
  <Cell N="LocPinX" V="1"><RefBy T="Page" ID="0"/></Cell>
  <Cell N="PinX" V="3.5" F="GUARD(3.5 in)" E="#REF!" U="IN"/>
  <Cell N="ThemeIndex" V="33"><?NewValue V?></Cell>
  <Cell N="Width" V="2" F="No Formula">kept text</Cell>
  <Cell N="pinx" V="1" Extra="1" Other="x&#10;y"/>
  <Section N="Geometry" IX="0" Del="0">
    <Cell N="NoFill" V="0"/>
    <Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0"/></Row>
    <Row T="LineTo" IX="2" Del="1"/>
    <Row T="LineTo" IX="3" LocalName="Linie"><Cell N="X" V="1" F="Width*1"/></Row>
  </Section>
  <Section N="Geometry" IX="1"><Row T="Ellipse" IX="1"><Cell N="X" V="1"/></Row></Section>
  <Section N="User" IX="4"><Row N="Row_1"><Cell N="Value" V="ü€" U="STR"/></Row><Row N="Row_2" Del="true"/></Section>
  <Section N="Scratch"><Row IX="0" N="both"><Cell N="A" V="1"/></Row></Section>
  <Section N="Tabs"><Row IX="0"><Cell N="Position0" V="1"/><Tab><Cell N="Position" V="2"/></Tab></Row></Section>
  <Trigger N="RecalcColor"><RefBy T="Page" ID="0"/></Trigger>
  <Text>  lead<cp IX="0"/>ab &amp; c<pp IX="1"/><tp IX="2"/><fld IX="0">3</fld><cp IX="x"/><zz a="b">in</zz><!--c--> tail  </Text>
  <Data1>one &lt; two</Data1>
  <Shapes>
    <Shape ID="2" MasterShape="4" Master="2" Del="0"><Cell N="X" V="1"/></Shape>
    <Shape ID="2" Type="Foreign" Del="1">
      <Cell N="ImgWidth" V="1"/>
      <ForeignData ForeignType="Bitmap" CompressionType="PNG" ObjectWidth="1" Extra="e">stray<Rel r:id="rId1"/></ForeignData>
    </Shape>
  </Shapes>
  <x:note xmlns:x="urn:z" a="1">hi<x:inner/></x:note>
  <!-- a comment between children -->
  <?pi data?>
</Shape>
<Shape ID="4294967295" Type="Shape" Del="1"/>
<Shape ID="4294967296" Master="x" Type="Guide"><Cell N="X" V="1"/></Shape>
"##;

fn rich_package() -> Vec<u8> {
    let styles = r##"<StyleSheet ID="1" Name="Normal" NameU="Normal" LineStyle="0" FillStyle="0" TextStyle="0" IsCustomName="1"><Cell N="LineWeight" V="0.01" F="THEMEVAL()"/><Section N="Character"><Row IX="0"><Cell N="Size" V="0.1666667" U="PT"/></Row></Section><Trigger N="RecalcColor"/><x:y xmlns:x="urn:q"/></StyleSheet><StyleSheet ID="2" NameU="B" LineStyle="1" FillStyle="x" TextStyle="007"><Cell N="A" V="1"/></StyleSheet>"##;
    VsdxBuilder::new()
        .more_styles(styles)
        .document_sheet(r#"<Cell N="DocLangID" V="1033"/><Section N="Reviewer"><Row IX="0"><Cell N="Name" V="x"/></Row></Section>"#)
        .master(MasterSpec::new(1, "M").shapes(r#"<Shape ID="4" Type="Shape" Master="9" MasterShape="9"><Cell N="PinX" V="1" F="Width/2"/></Shape><Shape ID="5"/>"#))
        .master(MasterSpec::new(2, "N").shapes(r#"<Shape ID="4"/>"#))
        .page(
            PageSpec::new(1, "P")
                .sheet(r#"<Cell N="PageWidth" V="8.5" U="IN"/><Section N="Layer"><Row IX="0"><Cell N="Name" V="Connector"/></Row></Section><Trigger N="RecalcColor"/>"#)
                .shapes(RICH_SHAPES)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3" Extra="x"/><Connect FromSheet="007" ToSheet="2"/>"#)
                .rel("rId1", &format!("{OD}image"), "../media/image1.png"),
        )
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", [1u8, 2, 3]))
        .build()
}

#[test]
fn a_package_with_every_known_part_kind_gives_back_the_same_text_for_every_cell_row_section_and_shape() {
    check_round_trip(&rich_package());
}

#[test]
fn of_preservation_the_attributes_of_a_cell_are_the_strings_the_file_gave() {
    // `V="1.50"` stays `1.50`, `1.#INF` stays, a formula's text stays, and an evaluation is no part of reading.
    let xml =
        r#"<Shape ID="1"><Cell N="A" V="1.50" F="Height*1.5" U="MM"/><Cell N="B" V="1.#INF"/><Cell N="C" V="-0.0"/><Cell N="D" V="  7 "/></Shape>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(xml)).build();
    let (doc, _) = read_ok(&bytes);
    let s = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    let emitted = shape(&s);
    assert_eq!(signature(&emitted), signature(&tree_of(&bytes, "/visio/pages/page1.xml").child("Shapes").unwrap().child("Shape").unwrap().clone()));
}

#[test]
fn of_preservation_a_foreign_attribute_and_element_keep_their_place_and_text() {
    let xml = r#"<Shape ID="5" x:mine="1" xmlns:x="urn:example"><Cell N="PinX" V="1"/><x:note>hi</x:note></Shape>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(xml)).build();
    check_round_trip(&bytes);
    let (doc, _) = read_ok(&bytes);
    let s = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    assert_eq!(s.raw.get("x:mine"), Some("1"));
    assert_eq!(s.raw.get("xmlns:x"), Some("urn:example"));
    assert_eq!(s.passthrough.iter().next().unwrap().after, 1);
}

#[test]
fn what_the_model_has_no_field_for_is_held_in_its_slot_and_not_lost_silently() {
    // `Text` has no attribute field in the model: its attribute is held in the text's own slot. The same holds at every position of a package (`drops.rs`
    // injects each kind of thing at each, and fails on one that is not in the model).
    let xml = r#"<Shape ID="1"><Text xml:space="preserve">a</Text></Shape>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(xml)).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(report.skipped_counts.len(), 0);
    // The text itself is read, and its attribute is held.
    let text = doc.pages.get(0).unwrap().shapes.get(0).unwrap().text.clone().expect("the text");
    assert_eq!(crate::held::dump(crate::held::rest(&text.ext.get().expect("the text's slot").vsdx, "Text")[0]), " xml:space=\"preserve\"");
}

// ------------------------------------------------------------------------------------------------------ a random package

/// A generator that needs no strategy machinery: the seed makes the file, and a failing seed is the reproducer.
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const VALUES: &[&str] =
    &["0", "1.50", "", "Themed", "themed", "#FF0000", "a&amp;b", "x&#10;y", "1.#INF", "-0.0", "  7 ", "ü€", "&lt;tag&gt;", "00012.50e0"];
const FORMULAS: &[&str] = &[
    "Inh",
    "No Formula",
    "Width*2",
    "SUM(1,2)",
    "1/0",
    "THEMEVAL()",
    "GUARD(2 in)",
    "Sheet.5!Width",
    "&quot;s&amp;t&quot;",
    "=1",
    "",
    "a b c",
    "(((",
];
const UNITS: &[&str] = &["IN", "MM", "NURBS", "POLYLINE", "x y", ""];
const ERRORS: &[&str] = &["#N/A", "#REF!", "#BOGUS", ""];
const CELL_NAMES: &[&str] = &["Angle", "FlipX", "Height", "PinX", "PinY", "Width", "X", "Y", "pinx", "Prop.Row_1", "Z"];
const NUMBERS: &[&str] = &["0", "1", "5", "4294967295", "4294967296", "007", "+3", "", "x", " 2", "-1"];

/// The unknown nodes a random file puts between its known children: an element of no vocabulary, a comment, an instruction, and a foreign
/// element. Each is a place the reader must keep (`Placed::after`), and the signature compares the sequence, so a node that comes back one place
/// off is a failure.
const UNKNOWNS: [&str; 4] = ["<zq a=\"1\">t</zq>", "<!--c-->", "<?p d?>", "<x:u xmlns:x=\"urn:z\"/>"];

/// An unknown node, or nothing, a time in `one_in`.
fn unknown(g: &mut Gen, one_in: usize) -> &'static str {
    if g.chance(one_in) { UNKNOWNS[g.below(UNKNOWNS.len())] } else { "" }
}

fn ends_with_unknown(text: &str) -> bool {
    UNKNOWNS.iter().any(|u| text.ends_with(u))
}

fn random_cell(g: &mut Gen, name: &str) -> String {
    let mut attrs = format!("N=\"{name}\"");
    if g.chance(2) {
        attrs += &format!(" F=\"{}\"", g.pick(FORMULAS));
    }
    if g.chance(2) {
        attrs += &format!(" V=\"{}\"", g.pick(VALUES));
    }
    if g.chance(3) {
        attrs += &format!(" U=\"{}\"", g.pick(UNITS));
    }
    if g.chance(4) {
        attrs += &format!(" E=\"{}\"", g.pick(ERRORS));
    }
    if g.chance(5) {
        attrs += " Extra=\"e&amp;x\" x:o=\"1\" xmlns:x=\"urn:q\"";
    }
    match g.below(8) {
        0 => format!("<Cell {attrs}>some &amp; text</Cell>"),
        1 => format!("<Cell {attrs}><?NewValue V?></Cell>"),
        2 => format!("<Cell {attrs}>before<RefBy T=\"Page\" ID=\"2\"/>after</Cell>"),
        _ => format!("<Cell {attrs}/>"),
    }
}

/// Unique cell names, in sorted order (the model keeps them so).
fn random_cells(g: &mut Gen) -> String {
    let mut names: Vec<&str> = CELL_NAMES.iter().copied().filter(|_| g.chance(3)).collect();
    names.sort_unstable();
    let mut out = unknown(g, 6).to_owned();
    for n in names {
        out += &random_cell(g, n);
        out += unknown(g, 6);
    }
    out
}

fn random_sections(g: &mut Gen) -> String {
    let mut out = String::new();
    let geometry = g.below(3);
    for ix in 0..geometry {
        out += &format!("<Section N=\"Geometry\" IX=\"{ix}\"{}>", if g.chance(4) { " Del=\"1\"" } else { "" });
        out += &random_cells(g);
        for r in 0..g.below(3) {
            out += &format!(
                "<Row T=\"{}\" IX=\"{r}\"{}>{}</Row>",
                g.pick(&["MoveTo", "LineTo", "Weird"]),
                if g.chance(4) { " Del=\"1\"" } else { "" },
                random_cells(g)
            );
            out += unknown(g, 6);
        }
        out += "</Section>";
    }
    if g.chance(2) {
        out += "<Section N=\"User\">";
        for r in 0..g.below(3) {
            out += &format!("<Row N=\"Row_{r}\"{}>{}</Row>", if g.chance(4) { " Del=\"true\"" } else { "" }, random_cells(g));
        }
        out += "</Section>";
    }
    if g.chance(3) {
        out += "<Section N=\"Scratch\">";
        for r in 0..g.below(3) {
            out += &format!("<Row IX=\"{r}\">{}</Row>", random_cells(g));
        }
        out += "</Section>";
    }
    if g.chance(4) {
        out += "<Section N=\"Mine\" IX=\"3\"><Row N=\"a\"><Cell N=\"x\" V=\"1\"/></Row></Section>";
    }
    out
}

fn random_text(g: &mut Gen) -> String {
    let mut out = String::from("<Text>");
    for _ in 0..g.below(6) {
        out += match g.below(8) {
            0 => "<cp IX=\"0\"/>",
            1 => "<pp IX=\"1\"/>",
            2 => "<tp IX=\"2\"/>",
            3 => "<fld IX=\"0\">3</fld>",
            4 => "<cp IX=\"x\"/>",
            5 => "<zz a=\"b\">in</zz>",
            6 => "&amp; some  text ",
            _ => "<!--c-->",
        };
    }
    out + "</Text>"
}

fn random_shape(g: &mut Gen, depth: usize) -> String {
    let mut attrs = format!("ID=\"{}\"", g.pick(NUMBERS));
    if g.chance(2) {
        attrs += &format!(" Type=\"{}\"", g.pick(&["Shape", "Group", "Foreign", "Guide", "Weird"]));
    }
    if g.chance(3) {
        attrs += &format!(" Master=\"{}\"", g.pick(NUMBERS));
    }
    if g.chance(3) {
        attrs += &format!(" MasterShape=\"{}\"", g.pick(NUMBERS));
    }
    if g.chance(4) {
        attrs += &format!(" Del=\"{}\"", g.pick(&["1", "0", "true", "x"]));
    }
    for link in ["LineStyle", "FillStyle", "TextStyle"] {
        if g.chance(4) {
            attrs += &format!(" {link}=\"{}\"", g.pick(NUMBERS));
        }
    }
    if g.chance(3) {
        attrs += &format!(" Name=\"n&amp;{}\" OriginalID=\"{}\"", g.below(9), g.below(9));
    }
    let mut inner = random_cells(g) + &random_sections(g);
    // A trigger is kept in the sheet's list and an unknown element in the shape's, and which of the two came first when nothing typed lies between
    // them is not held: so a trigger is never put right after an unknown node.
    if !ends_with_unknown(&inner) && g.chance(5) {
        inner += "<Trigger N=\"RecalcColor\"><RefBy T=\"Page\" ID=\"0\"/></Trigger>";
    }
    if g.chance(2) {
        inner += &random_text(g);
        inner += unknown(g, 5);
    }
    if g.chance(5) {
        inner += "<Data1>d &amp; e</Data1>";
        inner += unknown(g, 5);
    }
    if g.chance(6) {
        inner += "<ForeignData ForeignType=\"Bitmap\"><Rel r:id=\"rId9\"/></ForeignData>";
        inner += unknown(g, 5);
    }
    if depth < 3 && g.chance(3) {
        inner += "<Shapes>";
        for _ in 0..1 + g.below(3) {
            inner += &random_shape(g, depth + 1);
        }
        inner += "</Shapes>";
        inner += unknown(g, 5);
    }
    if g.chance(5) {
        inner += "<x:note xmlns:x=\"urn:z\" a=\"1\">hi</x:note>";
    }
    format!("<Shape {attrs}>{inner}</Shape>")
}

fn random_package(seed: u64) -> Vec<u8> {
    let mut g = Gen(seed | 1);
    let mut shapes_xml = String::new();
    for _ in 0..1 + g.below(4) {
        shapes_xml += &random_shape(&mut g, 0);
    }
    let master_shapes = random_shape(&mut g, 1);
    VsdxBuilder::new()
        .master(MasterSpec::new(1, "M").shapes(&master_shapes))
        .page(PageSpec::new(1, "P").sheet(&(random_cells(&mut g) + &random_sections(&mut g))).shapes(&shapes_xml))
        .build()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn a_random_package_gives_back_the_same_text_for_everything_it_held(seed in any::<u64>()) {
        check_round_trip(&random_package(seed));
    }
}
