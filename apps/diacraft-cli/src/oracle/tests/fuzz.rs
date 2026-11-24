//! Fuzzing the harness (AGENTS.md: importers are fuzzed, and the harness reads real files): a built package,
//! mutated at two levels, must be reported or refused and never panic or hang; the report must say the same twice; the cells it counts must add
//! up; and nothing the package holds in a name, a text or a formula may cross, whatever the mutation did to the XML around it.
//!
//! Mutating the archive's bytes is stopped by the container's checksums nearly always. To reach the reader and the comparer above it the XML of
//! each part is mutated and the archive written again with right checksums. **A clean run of this finds nothing a crafted input does not**
//! (25 million clean runs missed a 972 KB file that hung the container): the cost of a file's size is in `score.rs`, built by hand.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use diacraft_calc::Native;
use diacraft_testkit::package::{MasterSpec, OD, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::Limits;
use proptest::prelude::*;
use serde_json::Value;

use super::super::control::Control;
use super::super::file::report;
use super::native_control;

/// A package that has a little of everything the reader reads and the comparer scores, and a marker in every name, text and formula.
fn seed_package() -> Pkg {
    let shapes = r##"<Shape ID="1" Type="Group" Master="1" LineStyle="1" Name="SECRETSHAPE"><Cell N="Width" V="2" F="Height*2" U="IN"/><Cell N="Height" V="1"/><Cell N="X" V="1" F="Inh"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="1+1"/></Row></Section><Section N="User"><Row N="SECRETROW"><Cell N="Value" V="SECRETVALUE" F="&quot;SECRETTEXT&quot;&amp;Width" U="STR" E="#N/A"/></Row><Row N="b"><Cell N="Value" V="x" F="SECRETFUNC(1)"/></Row></Section><Section N="SECRETSECTION"><Row N="r"><Cell N="X" V="3" F="1+1"/></Row></Section><Text><cp IX="0"/>SECRETSHOWN<fld IX="0">SECRETFIELD</fld></Text><Data1>SECRETDATA</Data1><Shapes><Shape ID="2" MasterShape="4"/><Shape ID="3" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape></Shapes><Trigger N="RecalcColor"/><x:n xmlns:x="urn:z"/></Shape>"##;
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="C"><a:dk1><a:srgbClr val="000000"/></a:dk1></a:clrScheme><a:fontScheme name="F"><a:majorFont><a:latin typeface="X"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>"#;
    VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/>"##)
        .face_names(r#"<FaceName NameU="SECRETFONT" Panose="1"/>"#)
        .more_styles(r#"<StyleSheet ID="1" LineStyle="0" Name="SECRETSTYLE"><Cell N="LineWeight" V="1" F="THEMEVAL()"/></StyleSheet>"#)
        .document_sheet(r#"<Cell N="DocLangID" V="1033" F="1+1"/>"#)
        .document_extra("<DocumentSettings TopPage=\"0\"/><x:y xmlns:x=\"urn:q\"/>")
        .theme(theme.as_bytes())
        .master(
            MasterSpec::new(1, "SECRETMASTER")
                .shapes(r#"<Shape ID="4"><Cell N="PinX" V="1" F="Width/2"/><Cell N="Width" V="2" F="NOW()"/></Shape>"#)
                .icon("AAAB"),
        )
        .page(
            PageSpec::new(1, "SECRETPAGE")
                .shapes(shapes)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3"/>"#)
                .rel("rId1", &format!("{OD}image"), "../media/i.png"),
        )
        .page(PageSpec::new(2, "Q").attrs(r#" BackPage="1""#).without_part())
        .with(Pkg::new().part("/visio/media/i.png", "image/png", [1u8, 2, 3]))
        .pkg()
}

/// Fragments that change what an XML reader sees: a tag, an entity, a namespace, a quote, a number at an edge.
const SNIPPETS: &[&str] = &[
    "<",
    ">",
    "</",
    "/>",
    "&",
    "&amp;",
    "&#0;",
    "&#x110000;",
    "&bogus;",
    "\"",
    "'",
    "<![CDATA[",
    "]]>",
    "<!--",
    "-->",
    "<?",
    "?>",
    "<!DOCTYPE x>",
    "xmlns=\"urn:z\"",
    "xmlns:r=\"urn:z\"",
    " ID=\"4294967296\"",
    " ID=\"-1\"",
    " IX=\"",
    "<Cell N=\"X\"/>",
    "<Shape ID=\"1\">",
    "</Shape>",
    "<Shapes>",
    "</Shapes>",
    "<Section N=\"Geometry\"/>",
    "<Row/>",
    "\u{0}",
    "\u{FFFE}",
    "\u{1F600}",
    "\r\n",
    "1.#INF",
    "Themed",
    "Inh",
    "No Formula",
    "N=\"\"",
    " F=\"FILENAME()\"",
    " F=\"User.SECRETROW\"",
    " E=\"#REF!\"",
    " V=\"\"",
    " Del=\"1\"",
];

/// A tiny generator: the seed makes the mutation, so a failing seed reproduces.
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn mutate(g: &mut Gen, bytes: &mut Vec<u8>) {
    if bytes.is_empty() {
        bytes.extend_from_slice(b"<");
        return;
    }
    for _ in 0..1 + g.below(4) {
        let at = g.below(bytes.len());
        match g.below(7) {
            0 => bytes[at] ^= 1 << g.below(8),
            1 => {
                let end = (at + 1 + g.below(40)).min(bytes.len());
                bytes.drain(at..end);
            }
            2 => {
                let s = SNIPPETS[g.below(SNIPPETS.len())];
                bytes.splice(at..at, s.bytes());
            }
            3 => {
                let end = (at + 1 + g.below(200)).min(bytes.len());
                let chunk: Vec<u8> = bytes[at..end].to_vec();
                let to = g.below(bytes.len());
                bytes.splice(to..to, chunk);
            }
            4 => bytes.truncate(at),
            5 => {
                let other = g.below(bytes.len());
                bytes.swap(at, other);
            }
            _ => {
                let s = SNIPPETS[g.below(SNIPPETS.len())];
                let end = (at + 1 + g.below(10)).min(bytes.len());
                bytes.splice(at..end, s.bytes());
            }
        }
        if bytes.is_empty() {
            bytes.push(b'<');
        }
    }
}

fn control() -> &'static Control {
    static CONTROL: OnceLock<Control> = OnceLock::new();
    CONTROL.get_or_init(native_control)
}

/// Reports twice, within a bound, and checks what must hold of any outcome.
fn check(bytes: &[u8]) {
    let started = Instant::now();
    let first = report(bytes, &Limits::default(), Some("owner/repo"), &mut Native, control());
    let took = started.elapsed();
    // A hang guard and no measure of cost: a package the fuzz makes is reported in milliseconds, and a minute is generous at load 50.
    assert!(took < Duration::from_secs(60), "a package of {} bytes took {took:?}", bytes.len());
    let second = report(bytes, &Limits::default(), Some("owner/repo"), &mut Native, control());
    let (a, b) = (first.to_json().to_string(), second.to_json().to_string());
    assert_eq!(a, b, "reporting twice gave two reports");
    // Nothing the package holds in a name, a text or a formula crosses, whatever the mutation did around it.
    assert!(!a.to_ascii_uppercase().contains("SECRET"), "something from the file crossed: {a}");
    assert!(!a.contains("Height*2") && !a.contains("FILENAME()") && !a.contains("THEMEVAL()"), "a formula crossed: {a}");
    let json: Value = first.to_json().clone();
    match json["read"].as_bool() {
        Some(false) => {
            assert!(json["refusal"]["class"].is_string(), "{a}");
            assert_eq!(json["score"], Value::Null);
        }
        Some(true) => {
            let calc = &json["calc"];
            let n = |v: &Value| v.as_u64().unwrap_or(u64::MAX);
            let (compared, left_out, not_calculated, formula) =
                (n(&calc["compared"]), n(&calc["left_out_total"]), n(&calc["not_calculated"]["total"]), n(&calc["cells"]["formula"]));
            assert_eq!(compared + left_out + not_calculated, formula, "every formula cell is placed: {calc}");
            assert_eq!(formula + n(&calc["cells"]["not_formulas"]["total"]), n(&calc["cells"]["in_document"]), "{calc}");
            assert!(n(&calc["matched"]) <= compared, "{calc}");
            assert!(n(&calc["differences_listed"]) <= 50, "{calc}");
        }
        _ => panic!("a report that says neither: {a}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, failure_persistence: None, ..ProptestConfig::default() })]

    /// The XML of the parts, mutated and written again with right checksums.
    #[test]
    fn mutated_xml_in_a_package_with_right_checksums_is_reported_and_leaks_nothing(seed in any::<u64>()) {
        let mut g = Gen(seed | 1);
        let names = seed_package().part_names();
        let mut chosen = vec![names[g.below(names.len())].clone()];
        if g.below(3) == 0 {
            chosen.push(names[g.below(names.len())].clone());
        }
        let bytes = seed_package().map_parts(|name, part| {
            if chosen.iter().any(|c| c == name) {
                let mut inner = Gen(g.next() | 1);
                mutate(&mut inner, part);
            }
        }).build();
        check(&bytes);
    }

    /// The archive's own bytes, mutated: nearly always the container refuses, and the report says so.
    #[test]
    fn mutated_archive_bytes_are_reported_and_leak_nothing(seed in any::<u64>()) {
        let mut g = Gen(seed | 1);
        let mut bytes = seed_package().build();
        mutate(&mut g, &mut bytes);
        check(&bytes);
    }

    /// Anything at all.
    #[test]
    fn arbitrary_bytes_are_reported(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
        check(&bytes);
    }
}

#[test]
fn the_seed_package_reads_clean_and_scores_so_the_mutations_start_from_something() {
    let r = report(&seed_package().build(), &Limits::default(), None, &mut Native, control());
    assert!(r.refusal.is_none(), "{:?}", r.refusal);
    let calc = &r.to_json()["calc"];
    assert!(calc["compared"].as_u64().unwrap() >= 3, "{calc}");
    assert!(calc["differed"].as_u64().unwrap() >= 1, "a difference with secrets in it is listed: {calc}");
    assert!(calc["left_out_total"].as_u64().unwrap() >= 1 && calc["not_calculated"]["total"].as_u64().unwrap() >= 1, "{calc}");
}
