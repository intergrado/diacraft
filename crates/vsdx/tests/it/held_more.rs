//! The addendum from the review (item 5): tests for the paths of the reader's held part that no test reached. Each is built in code from the spec.
//!
//! * **The rows of the parts the model rebuilds** (a theme, `windows.xml`, the pages index): held with the model, and written back from it.
//! * **The role of a held part**: from the type of the relationship that reaches it first, else from its content type, else none.
//! * **When DiaCraft's solution is rebuilt and when it is held**: the index and the part each, by what they say beside DiaCraft's data.

use diacraft_doc::PartRole;
use diacraft_testkit::package::{CT_SOLUTION, CT_SOLUTIONS, NS_2012, NS_R, OD, PageSpec, Pkg, VIS, VsdxBuilder};
use diacraft_testkit::theme::ThemeXml;

use crate::model_writer::{DOC, MS, SIG, text_of, visit};
use crate::read_ok;

const WINDOWS: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Windows xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ClientWidth=\"1280\" ClientHeight=\"720\"><Window ID=\"0\" WindowType=\"Drawing\" Page=\"1\"/></Windows>";

#[test]
fn the_rows_of_a_theme_and_of_windows_xml_are_held_with_the_model_and_written_back_from_it() {
    let theme = ThemeXml::default().xml();
    let extra = Pkg::new()
        .part("/visio/windows.xml", "application/vnd.ms-visio.windows+xml", WINDOWS) // brand-ok: a content type the format defines
        .rel(DOC, "rId30", &format!("{VIS}windows"), "windows.xml")
        .rel("/visio/windows.xml", "rId1", &format!("{OD}image"), "media/w.png")
        .part_without_override("/visio/media/w.png", [0x89u8, b'P', b'N', b'G', 1, 2])
        .rel("/visio/theme/theme1.xml", "rId1", &format!("{OD}image"), "../media/t.png")
        .part_without_override("/visio/media/t.png", [0x89u8, b'P', b'N', b'G', 3, 4])
        .default_type("png", "image/png");
    let bytes = VsdxBuilder::new().theme(theme.as_bytes()).page(PageSpec::new(1, "P")).with(extra).build();
    let (doc, _) = read_ok(&bytes);
    let held = doc.vsdx.as_ref().unwrap();
    for (source, id) in [("/visio/windows.xml", "rId1"), ("/visio/theme/theme1.xml", "rId1")] {
        assert!(held.rels.iter().any(|r| &*r.source == source && &*r.id == id), "the row {id} of {source} is held with the model");
    }
    assert!(
        doc.parts.iter().any(|p| p.name.ends_with("w.png")) && doc.parts.iter().any(|p| p.name.ends_with("t.png")),
        "the parts only those rows reach are held"
    );
    let written = visit("rows of a theme and of windows.xml", &bytes);
    let pkg = crate::model_writer::package(&written.bytes);
    for (source, target) in [("/visio/windows.xml", "/visio/media/w.png"), ("/visio/theme/theme1.xml", "/visio/media/t.png")] {
        let rows = pkg.rels(source).unwrap();
        assert_eq!(rows.get("rId1").and_then(|r| r.target.as_deref()), Some(target), "the row of {source} is written");
    }
}

fn roles_of(bytes: &[u8]) -> Vec<(String, PartRole)> {
    let (doc, _) = read_ok(bytes);
    let mut roles: Vec<(String, PartRole)> = doc.parts.iter().map(|p| (p.name.to_string(), p.role)).collect();
    roles.sort_by(|a, b| a.0.cmp(&b.0));
    roles
}

#[test]
fn a_held_part_has_the_role_its_relationship_gives_it_else_the_one_its_content_type_gives_it() {
    let thing = "urn:example:thing";
    let extra = Pkg::new()
        // By the type of the relationship that reaches it.
        .part_without_override("/r/vbadata.bin", [1u8])
        .rel(DOC, "rId70", &format!("{MS}vbaData"), "../r/vbadata.bin")
        .part_without_override("/r/font.odttf", [2u8])
        .rel(DOC, "rId71", &format!("{OD}font"), "../r/font.odttf")
        .part_without_override("/r/project.bin", [3u8])
        .rel(DOC, "rId72", &format!("{MS}vbaProject"), "../r/project.bin")
        .part_without_override("/r/projectsig.bin", [4u8])
        .rel("/r/project.bin", "rId1", &format!("{MS}vbaProjectSignatureAgile"), "projectsig.bin")
        .part_without_override("/r/xmlsig.xml", "<Signature/>")
        .rel("/", "rId73", &format!("{SIG}signature"), "r/xmlsig.xml")
        // By the content type, the relationship that reaches it saying nothing.
        .part("/c/sig.xml", "application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml", "<Signature/>")
        .rel(DOC, "rId80", thing, "../c/sig.xml")
        .part("/c/vbasig.bin", "application/vnd.ms-office.vbaProjectSignature", [5u8])
        .rel(DOC, "rId81", thing, "../c/vbasig.bin")
        .part("/c/vba.bin", "application/vnd.ms-office.vbaProject", [6u8])
        .rel(DOC, "rId82", thing, "../c/vba.bin")
        .part("/c/ole.bin", "application/vnd.openxmlformats-officedocument.oleObject", [7u8])
        .rel(DOC, "rId83", thing, "../c/ole.bin")
        .part("/c/props.xml", "application/vnd.openxmlformats-officedocument.customXmlProperties+xml", "<p/>")
        .rel(DOC, "rId84", thing, "../c/props.xml")
        .part("/c/other.bin", "application/x-nothing", [8u8])
        .rel(DOC, "rId85", thing, "../c/other.bin");
    let roles = roles_of(&VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build());
    let role = |name: &str| roles.iter().find(|(n, _)| n == name).map(|(_, r)| *r).unwrap_or_else(|| panic!("{name} is not held: {roles:?}"));
    assert_eq!(role("/r/vbadata.bin"), PartRole::MacroData);
    assert_eq!(role("/r/font.odttf"), PartRole::Font);
    assert_eq!(role("/r/project.bin"), PartRole::MacroProject);
    assert_eq!(role("/r/projectsig.bin"), PartRole::MacroSignature, "any type that starts as the project signature's is one");
    assert_eq!(role("/r/xmlsig.xml"), PartRole::Signature);
    assert_eq!(role("/c/sig.xml"), PartRole::Signature);
    assert_eq!(role("/c/vbasig.bin"), PartRole::MacroSignature);
    assert_eq!(role("/c/vba.bin"), PartRole::MacroProject);
    assert_eq!(role("/c/ole.bin"), PartRole::Embedding);
    assert_eq!(role("/c/props.xml"), PartRole::CustomXml);
    assert_eq!(role("/c/other.bin"), PartRole::Other);
}

// ------------------------------------------------------------------------------------------------- DiaCraft's solution: rebuilt or held

const DATA: &str = r#"<dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:snap alignment_box="1"/></dc:Data>"#;

struct Case {
    name: &'static str,
    index_extra_root: &'static str,
    row_extra: &'static str,
    index_children: &'static str,
    part: String,
    part_rels: bool,
    second_index_row: bool,
    /// The index is held as a part; the part is.
    held: (bool, bool),
}

fn solution_package(case: &Case) -> Vec<u8> {
    let second = if case.second_index_row { "<Solution Name=\"OtherApp\"><Rel r:id=\"rId2\"/></Solution>" } else { "" };
    let index = format!(
        "<Solutions xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"{}><Solution Name=\"DiaCraft\"{}><Rel r:id=\"rId1\"/></Solution>{second}{}</Solutions>",
        case.index_extra_root, case.row_extra, case.index_children
    );
    let mut extra = Pkg::new()
        .part("/visio/solutions/solutions.xml", CT_SOLUTIONS, index)
        .rel(DOC, "rId60", &format!("{VIS}solutions"), "solutions/solutions.xml")
        .part("/visio/solutions/solution1.xml", CT_SOLUTION, &case.part)
        .rel("/visio/solutions/solutions.xml", "rId1", &format!("{VIS}solution"), "solution1.xml");
    if case.second_index_row {
        extra = extra.part("/visio/solutions/solution2.xml", CT_SOLUTION, "<SolutionXML Name=\"OtherApp\"><Data/></SolutionXML>").rel(
            "/visio/solutions/solutions.xml",
            "rId2",
            &format!("{VIS}solution"),
            "solution2.xml",
        );
    }
    if case.part_rels {
        extra = extra.part_without_override("/visio/solutions/side.bin", [1u8]).rel(
            "/visio/solutions/solution1.xml",
            "rId1",
            "urn:example:side",
            "side.bin",
        );
    }
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build()
}

#[test]
fn the_index_and_the_part_of_diacraft_s_solution_are_rebuilt_or_held_by_what_they_say_beside_the_data() {
    let pure = format!("<SolutionXML Name=\"DiaCraft\">{DATA}</SolutionXML>");
    let case = |name, f: &dyn Fn(&mut Case)| {
        let mut c = Case {
            name,
            index_extra_root: "",
            row_extra: "",
            index_children: "",
            part: pure.clone(),
            part_rels: false,
            second_index_row: false,
            held: (false, false),
        };
        f(&mut c);
        c
    };
    let cases = [
        case("an index of the one row and a part of the one data: both are the model's", &|_| {}),
        case("a row with an attribute beside its name", &|c| {
            c.row_extra = " x=\"1\"";
            c.held = (true, false);
        }),
        case("another program's solution in the index", &|c| {
            c.second_index_row = true;
            c.held = (true, false);
        }),
        case("an element of another vocabulary in the index", &|c| {
            c.index_children = "<q:Other xmlns:q=\"urn:q\"/>";
            c.held = (true, false);
        }),
        case("a declaration of another prefix on the index", &|c| {
            c.index_extra_root = " xmlns:q=\"urn:q\"";
            c.held = (true, false);
        }),
        case("an element beside the data in the part", &|c| {
            c.part = format!("<SolutionXML Name=\"DiaCraft\">{DATA}<Note/></SolutionXML>");
            c.held = (true, true);
        }),
        case("an attribute beside the name on the part", &|c| {
            c.part = format!("<SolutionXML Name=\"DiaCraft\" x=\"1\">{DATA}</SolutionXML>");
            c.held = (true, true);
        }),
        case("relationships of the part's own", &|c| {
            c.part_rels = true;
            c.held = (true, true);
        }),
    ];
    for c in &cases {
        let (doc, report) = read_ok(&solution_package(c));
        assert!(doc.diacraft.is_some(), "{}: {:?}", c.name, report.diacraft);
        let held = |name: &str| doc.parts.iter().any(|p| p.name.eq_ignore_ascii_case(name));
        assert_eq!(held("/visio/solutions/solutions.xml"), c.held.0, "{}: the index", c.name);
        assert_eq!(held("/visio/solutions/solution1.xml"), c.held.1, "{}: the part", c.name);
        // And a model save of the drawing writes both either way, with the data of the model.
        let written = crate::model_writer::model_write(&doc, false);
        assert!(text_of(&written.bytes, "/visio/solutions/solution1.xml").contains("alignment_box"), "{}", c.name);
        assert_eq!(read_ok(&written.bytes).0.diacraft, doc.diacraft, "{}", c.name);
    }
}

// ------------------------------------------------------------------------------------------------------------------- custom properties

#[test]
fn a_custom_part_that_declares_no_namespace_is_no_list_and_is_held_as_the_bytes_it_is() {
    // A `Properties` of no namespace is not the vocabulary's: a list written from it would give it the declaration, which is another part's content. It is held, as a part is.
    let custom = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Properties><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"2\" name=\"N\"><v>x</v></property></Properties>";
    let extra = Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", custom).rel(
        "/",
        "rId20",
        &format!("{OD}custom-properties"),
        "docProps/custom.xml",
    );
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build();
    let (doc, _) = read_ok(&bytes);
    assert!(doc.props.custom.is_empty(), "a part of no namespace is read as a list: {:?}", doc.props.custom);
    let held = doc.parts.iter().find(|p| p.name.eq_ignore_ascii_case("/docProps/custom.xml")).expect("the part is held");
    assert_eq!(&*held.bytes, custom.as_bytes());
    visit("a custom part of no namespace", &bytes);
    // The same part in the namespace of the vocabulary is the list, and is written as one.
    let declared = custom.replace("<Properties>", "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">").replace("<v>x</v>", "<vt:lpwstr>x</vt:lpwstr>");
    let extra = Pkg::new().part("/docProps/custom.xml", "application/vnd.openxmlformats-officedocument.custom-properties+xml", declared).rel(
        "/",
        "rId20",
        &format!("{OD}custom-properties"),
        "docProps/custom.xml",
    );
    let (doc, _) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build());
    assert_eq!(doc.props.custom.len(), 1);
}
