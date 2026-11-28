//! The `Value` of a `User` or `Field` row that holds a text says `STR` in every file a save writes, **whatever made the cell**.
//!
//! Gave the unit to the cells a build makes. The release gate then opened the files the CLI wrote of the demo drawings, which an earlier build made: the `User` rows of containers
//! and swimlanes in `.diacraft` files of 0.2.3 hold their text with no unit (`V="Container" F="&quot;Container&quot;"`, `V="frame"`, `V="phase"`), and the `Field` rows of a VSDX file of 0.2.3 hold
//! the cached text of an instance's own cell with none (`V="web-01" F="Inh"`). No `Type` cell in those rows says the value is a string, so the unit alone does, and the program that opens a file
//! says "an element in this file contains data that is invalid or inappropriate for the expected type of data" once for each such cell.
//!
//! * **A Save As from a `.diacraft` file** (the model writer) gives every such cell the unit, and the drawing becomes the model the file reads as, so the next save into the file holds them.
//! * **A save into a VSDX file DiaCraft wrote** gives its text the unit it lacks, whether or not the drawing was edited; **a file another program wrote** is kept as that program wrote it.
//! * **No other cell is given a unit**: a `Property` row's `Value` is typed by its `Type`, and the cells of a text kind carry none in the files of the program that opens them,
//!   so the trip of a file of another program through the model does not change them.
//!
//! Every test reads **the bytes a save wrote**: the files of the earlier build are made here, by taking the units out of the bytes a save wrote (as that build wrote them).

use diacraft_container::zip::{Archive, ArchiveWriter, Method};
use diacraft_engine::Session;
use diacraft_opc::xml::{El, Node, parse};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::edit::id_of;
use crate::native_common::Dir;
use crate::run;

/// The `Value` of every `User` and `Field` row, over every page, master and the document of a VSDX package, that holds a text and says no unit.
fn unitless_text_cells(bytes: &[u8]) -> Vec<String> {
    fn is_plain(v: &str) -> bool {
        let number =
            !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E')) && v.parse::<f64>().is_ok();
        let colour = v.strip_prefix('#').is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()));
        let date = v.len() >= 10 && v.get(4..5) == Some("-") && v.get(7..8) == Some("-") && v.bytes().take(4).all(|b| b.is_ascii_digit());
        v.is_empty() || number || colour || date || ["TRUE", "FALSE", "Themed", "1.#INF", "-1.#INF"].iter().any(|word| v.eq_ignore_ascii_case(word))
    }
    fn walk(el: &El, part: &str, section: &str, row: &str, out: &mut Vec<String>) {
        let (mut section, mut row) = (section.to_owned(), row.to_owned());
        if el.is("Section") {
            section = el.attr("N").unwrap_or("").to_owned();
        }
        if el.is("Row") {
            row = el.attr("N").or_else(|| el.attr("IX")).unwrap_or("").to_owned();
        }
        if el.is("Cell")
            && matches!(section.as_str(), "User" | "Field")
            && el.attr("N") == Some("Value")
            && let Some(v) = el.attr("V")
            && el.attr("U").is_none_or(|u| u.trim().is_empty())
            && !is_plain(v)
        {
            out.push(format!("{part} {section}/{row}/{} V={v:?} F={:?}", el.attr("N").unwrap_or(""), el.attr("F")));
        }
        for child in &el.children {
            if let Node::El(child) = child {
                walk(child, part, &section, &row, out);
            }
        }
    }
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let mut names: Vec<String> = package.parts().filter(|name| name.ends_with(".xml")).map(str::to_owned).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        if name.contains("/pages/page") || name.contains("/masters/master") || name.ends_with("/document.xml") {
            walk(&parse(package.part(&name).expect("the part")).expect("xml"), &name, "", "", &mut out);
        }
    }
    out
}

#[track_caller]
fn assert_every_text_says_str(what: &str, bytes: &[u8]) {
    let found = unitless_text_cells(bytes);
    assert!(found.is_empty(), "{what}: {} user and field values hold a text and say no unit:\n{}", found.len(), found.join("\n"));
}

/// The text of the part of a package whose name ends with `suffix`.
fn part(bytes: &[u8], suffix: &str) -> String {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let name = package.parts().find(|p| p.ends_with(suffix)).unwrap_or_else(|| panic!("no part ends with {suffix}")).to_owned();
    String::from_utf8(package.part(&name).expect("the part").to_vec()).expect("text")
}

/// The package `bytes` (a `.diacraft` file or a VSDX one) with each XML entry given to `f` (its name and its text), the entries written in the order they had.
fn rewritten(bytes: &[u8], f: impl Fn(&str, String) -> String) -> Vec<u8> {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).expect("an archive");
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut writer = ArchiveWriter::new();
    for (k, name) in names.iter().enumerate() {
        let data = archive.inflate(k, &mut diacraft_container::Work::default()).expect("an entry");
        let data = if name.ends_with(".xml") && !name.ends_with(".rels") && name != "[Content_Types].xml" {
            f(name, String::from_utf8(data).expect("text")).into_bytes()
        } else {
            data
        };
        let stored = name == "mimetype" || name.ends_with(".png");
        writer.add(name, &data, if stored { Method::Stored } else { Method::Deflated }).expect("an entry");
    }
    writer.finish().expect("an archive")
}

/// The text of the entry `name` of the archive `bytes` (a `.diacraft` file).
fn entry(bytes: &[u8], name: &str) -> String {
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).expect("an archive");
    let at = archive.position(name).unwrap_or_else(|| panic!("no entry {name}"));
    String::from_utf8(archive.inflate(at, &mut diacraft_container::Work::default()).expect("an entry")).expect("text")
}

/// `text` with the unit taken off the `Value` cell of every row of the sections `User` and `Field`: the text as the earlier build wrote those sections.
fn without_units_in_data_rows(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("<Section N=\"") {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        let end = from.find("</Section>").map_or(from.len(), |e| e + "</Section>".len());
        let (section, after) = from.split_at(end);
        let named = section.strip_prefix("<Section N=\"").unwrap_or("");
        if named.starts_with("User\"") || named.starts_with("Field\"") {
            out.push_str(&section.replace(" U=\"STR\"", ""));
        } else {
            out.push_str(section);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The unit of the `Value` of the first `msvStructureType` row a shape of the first page holds: `None` for a drawing with no such row, `Some(None)` for a cell that says no unit.
fn structure_unit(s: &Session) -> Option<Option<String>> {
    let page = s.document()?.pages.iter().find(|p| !p.background)?;
    page.shapes.iter().find_map(|shape| {
        let section = shape.sheet.sections.iter().find(|section| section.name.as_str() == "User")?;
        let row = section.rows.iter().find(|row| row.name.as_deref() == Some("msvStructureType"))?;
        row.cells.get("Value").map(|cell| cell.unit.as_deref().map(str::to_owned))
    })
}

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// A page with a container round two squares, a swimlane structure and the org chart's positions that hold fields: what the demo drawings of 0.2.3 hold.
fn drawing() -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"template": "organization_chart"}));
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "orgchart.executive", "page": 1, "x": 2.0, "y": 6.0})));
    run(&mut s, "shapedata.set", json!({"id": a, "row": "Name", "value": "Ann Lee"}));
    run(&mut s, "shapedata.set", json!({"id": a, "row": "Title", "value": "Chief Executive"}));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "orgchart.manager", "page": 1, "x": 5.0, "y": 6.0})));
    run(&mut s, "container.add", json!({"ids": [a, b], "heading": "Stage"}));
    run(&mut s, "swimlane.create", json!({}));
    s
}

/// The bytes of the `.diacraft` file `s` saves as, with every unit `STR` taken out of the parts of the drawing: the file as the build of 0.2.3 wrote it for the cells whose unit it did not write.
fn earlier_build(s: &mut Session, dir: &Dir) -> Vec<u8> {
    run(s, "file.save_as", json!({ "path": dir.path("now.diacraft") }));
    rewritten(&dir.read("now.diacraft"), |name, text| if name.ends_with(".xml") { text.replace(" U=\"STR\"", "") } else { text })
}

// ---- a Save As from a `.diacraft` file ---------------------------------------------------------------------------------------------------

#[test]
fn a_diacraft_file_of_an_earlier_build_saves_as_a_vsdx_file_whose_text_cells_say_str() {
    let dir = Dir::new();
    let mut maker = drawing();
    let old = earlier_build(&mut maker, &dir);
    let held = entry(&old, "pages/page1.xml");
    assert!(held.contains("N=\"msvStructureType\"") && !held.contains("U=\"STR\""), "the control: the earlier build's file says no unit:\n{held}");
    let path = dir.write("old.diacraft", &old);
    let mut s = opened(&path);
    run(&mut s, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    let bytes = dir.read("out.vsdx");
    assert_every_text_says_str("Save As of a .diacraft file of an earlier build", &bytes);
    // The cells the gate named, in the page the container and the swimlanes are on.
    let page = part(&bytes, "/pages/page1.xml");
    for name in ["msvStructureType", "DiaCraftContainerStyle", "DiaCraftSwimlane"] {
        assert!(page.contains(&format!("N=\"{name}\"")), "the control: the page holds the row {name}:\n{page}");
    }
    assert!(page.contains("V=\"Container\" U=\"STR\" F=\"&quot;Container&quot;\""), "{page}");
    assert!(page.contains("U=\"STR\" F=\"&quot;frame&quot;\"") || page.contains("V=\"frame\" U=\"STR\""), "{page}");
    // No other cell is given a unit: the label of a Shape Data row of the earlier build's masters is the text kind that the files of the program that opens them hold with none.
    let master = part(&bytes, "/masters/master1.xml");
    assert!(master.contains("<Cell N=\"Label\" V=\"Name\"/>"), "a text kind's cell stays as the earlier build wrote it:\n{master}");
    // A number a row holds, and the margin with its unit, are left as they are.
    assert!(page.contains("N=\"DiaCraftContainerMargin\"><Cell N=\"Value\" V=\"0.25\" U=\"IN\"/>"), "{page}");
}

#[test]
fn the_drawing_becomes_what_the_file_reads_as_so_the_next_save_into_it_keeps_the_units() {
    let dir = Dir::new();
    let mut maker = drawing();
    let old = earlier_build(&mut maker, &dir);
    let path = dir.write("old.diacraft", &old);
    let mut s = opened(&path);
    assert_eq!(structure_unit(&s), Some(None), "the control: the drawing holds the text of a container's row with no unit");
    run(&mut s, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    assert_eq!(structure_unit(&s), Some(Some("STR".to_owned())), "the drawing is the model the file reads as, with the unit");
    // An edit and a save into the file the Save As wrote: the cells of the page the edit did not touch are what the Save As wrote.
    let before = part(&dir.read("out.vsdx"), "/pages/page1.xml");
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.0}));
    run(&mut s, "file.save", json!({}));
    let bytes = dir.read("out.vsdx");
    assert_every_text_says_str("a save into the file the Save As wrote", &bytes);
    let after = part(&bytes, "/pages/page1.xml");
    let kept = "V=\"Container\" U=\"STR\" F=\"&quot;Container&quot;\"";
    assert!(before.contains(kept) && after.contains(kept), "{kept} is in both pages");
    // The file opens again as the drawing, and one more save with no edit changes nothing in the page.
    let mut again = opened(&dir.path("out.vsdx"));
    run(&mut again, "file.save", json!({}));
    assert_eq!(part(&dir.read("out.vsdx"), "/pages/page1.xml"), after, "an unedited save writes what it read");
}

// ---- a save into a file an earlier build wrote ---------------------------------------------------------------------------------------------

/// The VSDX file the drawing is saved as, with the unit taken off every cell of a `User` and a `Field` row: the file as 0.2.3 wrote it. It is written at `name` in `dir`.
fn earlier_vsdx(dir: &Dir, name: &str) -> Vec<u8> {
    let mut maker = drawing();
    run(&mut maker, "file.save_as", json!({ "path": dir.path("now.vsdx") }));
    let old = rewritten(&dir.read("now.vsdx"), |part, text| {
        if part.contains("pages/page") || part.contains("masters/master") { without_units_in_data_rows(&text) } else { text }
    });
    dir.write(name, &old);
    old
}

#[test]
fn a_save_into_a_vsdx_file_an_earlier_build_wrote_gives_its_text_cells_the_unit_they_lack() {
    let dir = Dir::new();
    let old = earlier_vsdx(&dir, "old.vsdx");
    let found = unitless_text_cells(&old);
    assert!(
        found.len() >= 4,
        "the control: the earlier build's file holds text with no unit in its user and field rows, and so says {} cells",
        found.len()
    );
    // Opened and saved with no edit at all, which is what the release gate did.
    let mut s = opened(&dir.path("old.vsdx"));
    run(&mut s, "file.save", json!({}));
    let bytes = dir.read("old.vsdx");
    assert_every_text_says_str("an unedited save into a file of an earlier build", &bytes);
    // And a Save As to another file, which splices into the file the drawing has.
    let dir2 = Dir::new();
    let mut s = opened(&dir.write("old2.vsdx", &old));
    run(&mut s, "file.save_as", json!({ "path": dir2.path("copy.vsdx") }));
    assert_every_text_says_str("a Save As of a file of an earlier build", &dir2.read("copy.vsdx"));
}

#[test]
fn a_file_another_program_wrote_is_kept_as_that_program_wrote_it() {
    // The cells of the earlier build's file in a package that names another program: nothing is added to a cell it wrote, with no edit.
    let cells = "<Cell N=\"PinX\" V=\"2\" U=\"IN\"/><Cell N=\"PinY\" V=\"2\" U=\"IN\"/><Cell N=\"Width\" V=\"1\" U=\"IN\"/><Cell N=\"Height\" V=\"1\" U=\"IN\"/>";
    let own = "<Section N=\"User\"><Row N=\"msvStructureType\"><Cell N=\"Value\" V=\"Container\" F=\"&quot;Container&quot;\"/></Row></Section>\
<Section N=\"Field\"><Row IX=\"0\"><Cell N=\"Value\" V=\"web-01\" F=\"Inh\"/><Cell N=\"Format\" V=\"@\" U=\"STR\"/></Row></Section>";
    let page = PageSpec::new(1, "Page-1").shapes(&shape(1, &format!("{cells}{own}")));
    let bytes = VsdxBuilder::new().page(page).build();
    let dir = Dir::new();
    let path = dir.write("theirs.vsdx", &bytes);
    let before = part(&bytes, "/pages/page1.xml");
    let mut s = opened(&path);
    run(&mut s, "file.save", json!({}));
    assert_eq!(part(&dir.read("theirs.vsdx"), "/pages/page1.xml"), before, "a file another program wrote, saved with no edit, keeps its cells");
    assert!(!unitless_text_cells(&dir.read("theirs.vsdx")).is_empty(), "the control: the cells are still the other program's");
}
