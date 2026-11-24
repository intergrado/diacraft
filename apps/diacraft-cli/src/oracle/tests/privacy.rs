//! What crosses (AGENTS.md): a hash, a category, a score, names from the spec's lists, an error class, expected and actual values. No formula
//! text, no text content, and no name a person made up.

use std::sync::Arc;

use diacraft_calc::{Calculator, CellAddress, CellEval, CellKind, Evaluator, Native, Value};
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::Limits;

use super::super::file::report;
use super::native_control;

/// A calculator that works every cell out as the evaluator does and then gives a text full of the marker as the result, so that the
/// calculated side of a difference is exercised with something that must not cross.
struct Secret;

impl Calculator for Secret {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula && e.unsupported.is_none() {
            e.raw = Value::text("SECRETACTUAL");
            e.error = None;
        }
        Arc::new(e)
    }
}

#[test]
fn no_formula_and_no_text_crosses() {
    let shape_xml = r##"<Shape ID="1" Name="SECRETNAME" NameU="SECRETNAMEU" Type="Group">
        <Cell N="SECRETCELL" V="SECRETVALUE" F="SECRETFUNC(1)" U="SECRETUNIT" E="SECRETERR"/>
        <Cell N="Width" V="SECRETWIDTH" F="SECRETFUNC2(&quot;SECRETSTRING&quot;)&amp;Width"/>
        <Cell N="Height" V="SECRETHEIGHT" F="Sheet.SECRET!Width" U="SECRETU"/>
        <Cell N="PinX" V="5" F="IF(1,1,SECRETFUNC3())"/>
        <Cell N="PinY" V="SECRETPINY" F="1+1"/>
        <Section N="User"><Row N="SECRETROW"><Cell N="Value" V="SECRETROWVALUE" F="&quot;SECRETFORMULATEXT&quot;" U="STR"/><Cell N="Prompt" V="SECRETPROMPT"/></Row></Section>
        <Section N="SECRETSECTION"><Row N="SECRETROW2"><Cell N="X" V="SECRETX" F="1+1"/></Row></Section>
        <Section N="Property"><Row N="SECRETPROP"><Cell N="Label" V="SECRETLABEL" F="&quot;SECRETLABELF&quot;" U="STR"/></Row></Section>
        <Trigger N="SECRETTRIGGER"/>
        <Text>SECRETTEXT<fld IX="0">SECRETFIELD</fld></Text><Data1>SECRETDATA</Data1>
        <x:Foreign xmlns:x="urn:SECRETNS">SECRETFOREIGN</x:Foreign></Shape>"##;
    let bytes = VsdxBuilder::new()
        .app("<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>SECRETAPP</Application><Company>SECRETCOMPANY</Company></Properties>")
        .core("<cp:coreProperties xmlns:cp=\"urn:x\" xmlns:dc=\"urn:y\"><dc:title>SECRETTITLE</dc:title><dc:creator>SECRETAUTHOR</dc:creator></cp:coreProperties>")
        .master(MasterSpec::new(1, "SECRETMASTER").shapes(r#"<Shape ID="4"><Cell N="SECRETM" V="SECRETMV" F="SECRETMF()"/></Shape>"#))
        .page(PageSpec::new(1, "SECRETPAGE").shapes(shape_xml).sheet(r#"<Cell N="SECRETPAGECELL" V="SECRETPV" F="1+1"/>"#))
        .more_styles(r#"<StyleSheet ID="9" Name="SECRETSTYLE" NameU="SECRETSTYLEU"><Cell N="SECRETSC" V="SECRETSV" F="SECRETSF()"/></StyleSheet>"#)
        .face_names(r#"<FaceName NameU="SECRETFONT"/>"#)
        .document_sheet(r#"<Cell N="SECRETDOCCELL" V="SECRETDV" F="1+1"/>"#)
        .solution("SECRETSOLUTION", "<SolutionXML Name=\"SECRETSOLUTION\"><SECRETELEMENT/></SolutionXML>")
        .with(Pkg::new().part("/visio/part.bin", "application/x-secret", b"SECRETBYTES"))
        .build();
    // With the evaluator, and with a calculator whose results are text full of the marker.
    let control = native_control();
    let plain = report(&bytes, &Limits::default(), Some("owner/repo"), &mut Native, &control);
    assert!(plain.to_json().get("category").is_some(), "the file reads: {:?}", plain.refusal);
    let with_secret = report(&bytes, &Limits::default(), Some("owner/repo"), &mut Secret, &control);
    for r in [&plain, &with_secret] {
        let text = r.to_json().to_string().to_ascii_uppercase();
        assert!(!text.contains("SECRET"), "something from the file crossed: {}", &text[text.find("SECRET").unwrap().saturating_sub(40)..][..90]);
    }
    // And both were exercised: differences are listed, with the values by kind.
    let diffs = plain.to_json()["calc"]["differences"].as_array().unwrap().clone();
    assert!(diffs.len() >= 4, "{diffs:?}");
    assert!(diffs.iter().any(|d| d["expected"]["v"].as_str().is_some_and(|v| v.starts_with("<text:"))), "a text is its length: {diffs:?}");
    let secret_diffs = with_secret.to_json()["calc"]["differences"].as_array().unwrap().clone();
    assert!(secret_diffs.iter().any(|d| d["actual"]["v"] == "<text:12>"), "SECRETACTUAL is twelve characters: {secret_diffs:?}");
    assert!(
        plain.to_json()["calc"]["not_calculated"]["unsupported_unlisted"].as_u64().unwrap() >= 1,
        "and the function the spec does not list is counted"
    );
}

#[test]
fn a_name_the_spec_lists_crosses_and_a_name_it_does_not_is_the_word_for_unlisted() {
    let cells = r#"<Cell N="Width" V="3" F="1+1"/><Cell N="MadeUpCell" V="3" F="1+1"/><Section N="User"><Row N="Mine"><Cell N="Value" V="3" F="1+1"/></Row></Section>"#;
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&diacraft_testkit::package::shape(1, cells))).build();
    let json = report(&bytes, &Limits::default(), None, &mut Native, &native_control()).to_json().clone();
    let by_cell = &json["calc"]["by_cell"];
    assert_eq!(by_cell["Width"]["differed"], 1);
    assert_eq!(by_cell["(unlisted)"]["differed"], 1, "{by_cell}");
    assert_eq!(by_cell["User.Value"]["differed"], 1, "a row's cell is named by its section and its own name, never the row's");
    let diffs = json["calc"]["differences"].as_array().unwrap();
    assert!(diffs.iter().any(|d| d["cell"] == "Width"));
    assert!(diffs.iter().any(|d| d["cell"].is_null() && d["section"].is_null()), "the unlisted cell has no name: {diffs:?}");
    assert!(!json.to_string().contains("MadeUpCell") && !json.to_string().contains("Mine"));
}
