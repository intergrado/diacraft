//! **The text of the `User` and `Field` rows of a file another program wrote is written as it was read, on every route**.
//!
//! Gave the `Value` of a `User` or `Field` row that holds a text the unit `STR` in every file DiaCraft writes. The reference recheck of 0.2.4 found that the model route did it for a
//! drawing that came out of the package of another program: a file whose extended properties name another program (any program that is not DiaCraft), with a `User` row whose `Value` is
//! a text and says no unit, gained `U="STR"` on the trip through `.diacraft` (the content check of the trip counted one difference and its model check turned false, and one file of the corpus was
//! changed so). A save into the file kept the cell as it was. **The units are DiaCraft's to state in a drawing that came from no VSDX package, or from one whose extended properties name DiaCraft**
//! (`diacraft_doc::string_unit::units_are_ours_to_state`), and in no other: the cell is the other program's.
//!
//! **A date field** (a `Field` row whose `Type` is 5) is typed by its `Type`, and its cached value is a date's: a text that no date can be read from, in a drawing that holds no time, stays as it was
//! read (it was given `STR` by the same pass, where the files hold `U` `DATE` and a date), and a text a date is read from is the date with `DATE`.
//!
//! Every test reads the bytes of the file a save wrote.

use diacraft_engine::Session;
use diacraft_opc::xml::{El, Node, parse};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, cell, shape, shape_with};
use serde_json::json;

use crate::native_common::Dir;
use crate::odg_build::{DRAWING, Drawing, package, rect};
use crate::run;

/// `V`, `U` and `F` of a cell.
type Said = (Option<String>, Option<String>, Option<String>);

/// The `Value` cells of the `User` rows in every page and master of the package `bytes`, as `(part, row, V, U, F)`.
fn user_values(bytes: &[u8]) -> Vec<(String, String, Said)> {
    fn walk(el: &El, part: &str, section: &str, row: &str, out: &mut Vec<(String, String, Said)>) {
        let (mut section, mut row) = (section.to_owned(), row.to_owned());
        if el.is("Section") {
            section = el.attr("N").unwrap_or("").to_owned();
        }
        if el.is("Row") {
            row = el.attr("N").or_else(|| el.attr("IX")).unwrap_or("").to_owned();
        }
        if el.is("Cell") && section == "User" && el.attr("N") == Some("Value") {
            let get = |a: &str| el.attr(a).map(str::to_owned);
            out.push((part.to_owned(), row.clone(), (get("V"), get("U"), get("F"))));
        }
        for child in &el.children {
            if let Node::El(child) = child {
                walk(child, part, &section, &row, out);
            }
        }
    }
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let mut names: Vec<String> =
        package.parts().filter(|name| name.contains("/pages/page") || name.contains("/masters/master")).map(str::to_owned).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        walk(&parse(package.part(&name).expect("the part")).expect("xml"), &name, "", "", &mut out);
    }
    out
}

fn app(name: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>{name}</Application><AppVersion>16.0000</AppVersion></Properties>"
    )
}

/// The three forms of the row of the vector: a string formula, a function that gives a text, and no formula. `Value` says no unit in each.
fn user_row(kind: char) -> &'static str {
    match kind {
        'F' => {
            "<Section N=\"User\"><Row N=\"Alder\"><Cell N=\"Value\" V=\"Birch\" F=\"&quot;Birch&quot;\"/><Cell N=\"Prompt\" V=\"\"/></Row></Section>"
        }
        'T' => {
            "<Section N=\"User\"><Row N=\"Alder\"><Cell N=\"Value\" V=\"Birch\" F=\"FONTTOID(&quot;Lato&quot;)\"/><Cell N=\"Prompt\" V=\"\"/></Row></Section>"
        }
        _ => "<Section N=\"User\"><Row N=\"Alder\"><Cell N=\"Value\" V=\"Birch\"/><Cell N=\"Prompt\" V=\"\"/></Row></Section>",
    }
}

/// What the row of [`user_row`] says, as a test compares it.
fn birch(kind: char) -> Said {
    let formula = match kind {
        'F' => Some("\"Birch\"".to_owned()),
        'T' => Some("FONTTOID(\"Lato\")".to_owned()),
        _ => None,
    };
    (Some("Birch".to_owned()), None, formula)
}

fn units() -> String {
    format!("{}{}{}{}", cell("PinX", "2"), cell("PinY", "2"), cell("Width", "1"), cell("Height", "1"))
}

/// A package whose extended properties are `properties`, with the row `kind` on the shape of the master (`where_` `'M'`) or on the page's shape (`'P'`).
fn package_with(properties: &str, where_: char, kind: char) -> VsdxBuilder {
    let (on_master, on_page) = if where_ == 'M' { (user_row(kind), "") } else { ("", user_row(kind)) };
    VsdxBuilder::new()
        .app(properties)
        .master(MasterSpec::new(1, "Box").shapes(&shape(5, &format!("{}{on_master}", units()))))
        .page(PageSpec::new(1, "Page-1").shapes(&shape_with(11, " Type=\"Shape\" Master=\"1\"", &format!("{}{on_page}", units()))))
}

/// The package taken through `.diacraft` and written as a VSDX file again: the trip the content check of the corpus makes.
fn through_diacraft(bytes: &[u8], dir: &Dir) -> Vec<u8> {
    let path = dir.write("in.vsdx", bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("between.diacraft") }));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("between.diacraft") }));
    run(&mut again, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    dir.read("out.vsdx")
}

/// The package opened and saved with no edit: a save into the file.
fn splice(bytes: &[u8], dir: &Dir) -> Vec<u8> {
    let path = dir.write("spliced.vsdx", bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    run(&mut s, "file.save", json!({}));
    dir.read("spliced.vsdx")
}

#[test]
fn the_text_of_a_user_row_of_another_programs_file_keeps_no_unit_through_a_diacraft_file() {
    for program in ["Other Writer 9", "Quill Draw 3"] {
        for where_ in ['M', 'P'] {
            for kind in ['F', 'T', 'N'] {
                let bytes = package_with(&app(program), where_, kind).build();
                let dir = Dir::new();
                let found = user_values(&through_diacraft(&bytes, &dir));
                let want = vec![(
                    format!("{}/{}", diacraft_testkit::package::DIR, if where_ == 'M' { "masters/master1.xml" } else { "pages/page1.xml" }),
                    "Alder".to_owned(),
                    birch(kind),
                )];
                let found: Vec<_> = found.into_iter().map(|(part, row, said)| (part.to_ascii_lowercase(), row, said)).collect();
                assert_eq!(found, want, "{program}, on the {where_} shape, {kind}: the cell is the other program's and says no unit");
            }
        }
    }
}

#[test]
fn and_a_save_into_the_file_keeps_it_the_same() {
    for where_ in ['M', 'P'] {
        for kind in ['F', 'T', 'N'] {
            let bytes = package_with(&app("Other Writer 9"), where_, kind).build();
            let dir = Dir::new();
            let found = user_values(&splice(&bytes, &dir));
            assert_eq!(found.iter().map(|(_, _, said)| said.clone()).collect::<Vec<_>>(), vec![birch(kind)], "{where_} {kind}");
        }
    }
}

#[test]
fn a_package_that_names_no_program_is_another_programs_too() {
    let bytes =
        package_with(&app("x"), 'P', 'F').pkg().without_rels_of_type("/", &format!("{}extended-properties", diacraft_testkit::package::OD)).build();
    let dir = Dir::new();
    let found = user_values(&through_diacraft(&bytes, &dir));
    assert_eq!(found.iter().map(|(_, _, said)| said.clone()).collect::<Vec<_>>(), vec![birch('F')], "no `Application` is no DiaCraft");
}

#[test]
fn a_file_that_names_diacraft_is_given_the_unit_on_both_routes() {
    for where_ in ['M', 'P'] {
        for kind in ['F', 'T', 'N'] {
            let bytes = package_with(&app("DiaCraft"), where_, kind).build();
            let (dir, other) = (Dir::new(), Dir::new());
            let said = |bytes: &[u8]| user_values(bytes).into_iter().map(|(_, _, said)| said).collect::<Vec<_>>();
            let (_, _, formula) = birch(kind);
            let with_unit = vec![(Some("Birch".to_owned()), Some("STR".to_owned()), formula)];
            assert_eq!(said(&through_diacraft(&bytes, &dir)), with_unit, "the model route, {where_} {kind}");
            assert_eq!(said(&splice(&bytes, &other)), with_unit, "a save into the file, {where_} {kind}");
        }
    }
}

// ---- a date field --------------------------------------------------------------------------------------------------------------------------

/// A rectangle whose text is a date field as a producer caches it: the text can be a date this program reads (`11/27/2026 3:28 PM`) or one it cannot (`27 novembre 2026`).
fn date_field(shown: &str) -> String {
    format!(
        r#"<draw:custom-shape draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="fields" svg:width="9cm" svg:height="2cm" svg:x="8cm" svg:y="3cm"><text:p>on <text:date>{shown}</text:date></text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="non-primitive" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
    )
}

fn odg_with(shown: &str, meta: &str) -> Vec<u8> {
    let d = Drawing::new().meta(meta).page(&format!("{}{}", rect("box", (2.0, 3.0, 4.0, 2.0), ""), date_field(shown)));
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), d.content_xml().into_bytes()),
            ("styles.xml".to_owned(), d.styles_xml().into_bytes()),
            ("meta.xml".to_owned(), d.meta_xml().into_bytes()),
        ],
    )
}

/// The `Type`, `Value` and its `U` of the one `Field` row of the file `bytes`.
fn the_date_row(bytes: &[u8]) -> (Option<String>, Said) {
    fn find<'a>(el: &'a El, out: &mut Vec<&'a El>) {
        if el.is("Section") && el.attr("N") == Some("Field") {
            out.push(el);
        }
        for c in &el.children {
            if let Node::El(child) = c {
                find(child, out);
            }
        }
    }
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let name = package.parts().find(|p| p.ends_with("/pages/page1.xml")).expect("a page").to_owned();
    let root = parse(package.part(&name).expect("the part")).expect("xml");
    let mut sections = Vec::new();
    find(&root, &mut sections);
    let row = sections.first().and_then(|s| s.elements().find(|r| r.is("Row"))).expect("a field row");
    let get =
        |name: &str, attr: &str| row.elements().find(|c| c.is("Cell") && c.attr("N") == Some(name)).and_then(|c| c.attr(attr)).map(str::to_owned);
    (get("Type", "V"), (get("Value", "V"), get("Value", "U"), get("Value", "F")))
}

#[test]
fn a_date_field_whose_text_no_date_is_read_from_is_not_given_str() {
    // No time in the drawing, and a text that is no date this program reads: nothing says the date, so the cell stays as it was read (no unit), as before, and is not a string.
    let dir = Dir::new();
    let path = dir.write("fields.odg", &odg_with("27 novembre 2026", ""));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("across.vsdx") }));
    let (kind, value) = the_date_row(&dir.read("across.vsdx"));
    assert_eq!(kind.as_deref(), Some("5"), "a date");
    assert_eq!(
        value,
        (Some("27 novembre 2026".to_owned()), None, Some("NOW()".to_owned())),
        "a date field's cached value is a date's, never a string's"
    );
    // The same through a `.diacraft` file: the first save stamped the drawing with its time, so the date is the one the save gave it with the unit of a date, and never a string.
    run(&mut s, "file.save_as", json!({ "path": dir.path("between.diacraft") }));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("between.diacraft") }));
    run(&mut again, "file.save_as", json!({ "path": dir.path("from-native.vsdx") }));
    let (_, value) = the_date_row(&dir.read("from-native.vsdx"));
    assert_ne!(value.1.as_deref(), Some("STR"), "{value:?}");
}

#[test]
fn a_date_field_whose_text_is_a_date_keeps_the_unit_of_a_date() {
    for meta in ["", "<meta:creation-date>2026-03-01T09:00:00</meta:creation-date><dc:date>2026-03-04T10:30:00</dc:date>"] {
        let dir = Dir::new();
        let path = dir.write("fields.odg", &odg_with("11/27/2026 3:28 PM", meta));
        let mut s = Session::new();
        run(&mut s, "file.open", json!({ "path": path }));
        run(&mut s, "file.save_as", json!({ "path": dir.path("across.vsdx") }));
        let (kind, (v, u, f)) = the_date_row(&dir.read("across.vsdx"));
        assert_eq!(
            (kind.as_deref(), v.as_deref(), u.as_deref(), f.as_deref()),
            (Some("5"), Some("2026-11-27T15:28:00"), Some("DATE"), Some("NOW()")),
            "{meta:?}"
        );
    }
}
