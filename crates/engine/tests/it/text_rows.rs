//! **A text row past the first is whole in every VSDX file DiaCraft saves**.
//!
//! A style holds row 0 alone, so a reader of a VSDX file takes a cell that a `Character` or `Paragraph` row of `IX` 1 or more lacks from the master's row of that `IX` and from nowhere else, and reads
//! the default of the cell's kind, 0, where there is none. The reference application, opening files DiaCraft wrote, drew the title line of every org chart position as letters piled on one another:
//! the master's `Character` row `IX` 1 held `Size` and `Style` alone, so its `FontScale` read 0 (letters of no width) and its `Font` read 0. The text of a language sample and of drawings converted
//! from an `.odg` did the same, for the same reason: a row past the first that the writer of the run or the `.odg` reader made with some of its cells.
//!
//! * **The vectors**: the title row of each org chart position holds the 12 cells of its section in `diacraft_doc::root_text::row_cells`, with the title's size and weight and the values the
//!   renderer assumes; the runs of a language sample (a face, a size, a bold run, an italic run, a language) in a file whose root states nothing; the runs and paragraphs of an `.odg` saved as VSDX.
//! * **The sweeps**: every master of the built-in stencils with text, and text formatted by `text.format` with each property, hold no row past the first that lacks a cell, and no cell out of the
//!   range its specification gives; every marker names a row a reader finds.
//! * **The routes**: a new drawing (the model writer), a file a drawing was read from (the splice), a `.diacraft` file of an older build, an `.odg`; and a save of each with no edit is the same bytes.

use std::collections::BTreeMap;

use diacraft_container::xml::{self, El};
use diacraft_doc::root_text::{RootCell, row_cells};
use diacraft_engine::Session;
use diacraft_engine::stencil::all_masters;
use serde_json::{Value, json};

use diacraft_testkit::package::{MasterSpec, PageSpec, ROOT_STYLE, VsdxBuilder};

use crate::edit::id_of;
use crate::native_common::{Dir, opened, pinned};
use crate::run;

// ------------------------------------------------------------------------------------------------------------------------------- the file

/// The parts of a VSDX package this file reads (spelled in two pieces, as the other tests do, so that no line of the tree holds the format's directory name).
const PAGES_DIR: &str = concat!("/vi", "sio", "/pages/");
const MASTERS_DIR: &str = concat!("/vi", "sio", "/masters/");
const MASTERS_INDEX: &str = concat!("/vi", "sio", "/masters/masters.xml");
const MASTERS_RELS: &str = concat!("/vi", "sio", "/masters/_rels/masters.xml.rels");
const DOCUMENT: &str = concat!("/vi", "sio", "/document.xml");

/// The sections whose rows this file looks at.
const TEXT: [&str; 2] = ["Character", "Paragraph"];

/// A cell as the file holds it: `V`, `F` and `U`.
#[derive(Clone, Debug, PartialEq)]
struct Held {
    v: Option<String>,
    f: Option<String>,
    u: Option<String>,
}

/// A row of a text section, as a saved part holds it.
#[derive(Clone, Debug)]
struct TextRow {
    section: String,
    ix: u32,
    del: bool,
    cells: BTreeMap<String, Held>,
}

/// A shape of a saved part that holds text rows or a text.
#[derive(Clone, Debug)]
struct SavedShape {
    /// The part it is in.
    part: String,
    id: String,
    /// How deep it is in its part: 0 for a shape of the page or of the master.
    depth: usize,
    /// The master's part and the shape there this one stands for (`MasterShape`; `None` for the master's top-level shape): where a reader merges a row of the same `IX`.
    stands_for: Option<(String, Option<String>)>,
    rows: Vec<TextRow>,
    /// The markers of its own `Text`, as `('c' or 'p', IX)`; `None` when it holds no text of its own.
    markers: Option<Vec<(char, u32)>>,
}

impl SavedShape {
    fn at(&self) -> String {
        format!("{} shape {}", self.part, self.id)
    }

    fn row(&self, section: &str, ix: u32) -> Option<&TextRow> {
        self.rows.iter().find(|r| r.section == section && r.ix == ix && !r.del)
    }
}

/// What a saved package holds of text: its shapes, and the faces of its font table.
struct Saved {
    shapes: Vec<SavedShape>,
    faces: Vec<String>,
}

impl Saved {
    fn read(bytes: &[u8]) -> Saved {
        let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
        let masters = master_parts(&pkg);
        let mut shapes = Vec::new();
        let names: Vec<String> = pkg
            .parts()
            .filter(|p| (p.starts_with(&format!("{PAGES_DIR}page")) || p.starts_with(&format!("{MASTERS_DIR}master"))) && p.ends_with(".xml"))
            .map(str::to_owned)
            .collect();
        for name in names {
            let root = xml::parse(pkg.part(&name).unwrap()).unwrap();
            if let Some(list) = root.child("Shapes") {
                walk(list, &name, 0, None, &masters, &mut shapes);
            }
        }
        let faces = pkg
            .part(DOCUMENT)
            .map(|d| xml::parse(d).unwrap())
            .and_then(|d| d.child("FaceNames").map(|f| f.children_named("FaceName").filter_map(|n| n.attr("NameU").map(str::to_owned)).collect()))
            .unwrap_or_default();
        Saved { shapes, faces }
    }

    /// The shape a shape stands for in its master's part.
    fn master_of(&self, shape: &SavedShape) -> Option<&SavedShape> {
        let (part, id) = shape.stands_for.as_ref()?;
        self.shapes.iter().find(|m| m.part == *part && id.as_ref().map_or(m.depth == 0, |id| m.id == *id))
    }

    /// The shape `id` of the first page.
    fn page_shape(&self, id: u32) -> &SavedShape {
        let page = format!("{PAGES_DIR}page1.xml");
        self.shapes.iter().find(|s| s.part == page && s.id == id.to_string()).unwrap_or_else(|| panic!("no shape {id} on the first page"))
    }
}

/// The part of each master of a package, by the master's `ID`.
fn master_parts(pkg: &diacraft_opc::Package) -> BTreeMap<String, String> {
    let (Some(index), Some(rels)) = (pkg.part(MASTERS_INDEX), pkg.part(MASTERS_RELS)) else {
        return BTreeMap::new();
    };
    let (index, rels) = (xml::parse(index).unwrap(), xml::parse(rels).unwrap());
    let targets: BTreeMap<&str, &str> = rels.elements().filter_map(|r| Some((r.attr("Id")?, r.attr("Target")?))).collect();
    index
        .elements()
        .filter_map(|m| {
            let target = targets.get(m.child("Rel")?.attr("r:id")?)?;
            Some((m.attr("ID")?.to_owned(), format!("{MASTERS_DIR}{target}")))
        })
        .collect()
}

fn walk(list: &El, part: &str, depth: usize, instance_master: Option<&String>, masters: &BTreeMap<String, String>, out: &mut Vec<SavedShape>) {
    for shape in list.children_named("Shape") {
        let own_master = shape.attr("Master").and_then(|m| masters.get(m));
        let master = own_master.or(instance_master);
        let stands_for = match (own_master, instance_master, shape.attr("MasterShape")) {
            (Some(m), _, ms) => Some((m.clone(), ms.map(str::to_owned))),
            (None, Some(m), Some(ms)) => Some((m.clone(), Some(ms.to_owned()))),
            _ => None,
        };
        let mut rows = Vec::new();
        for section in shape.children_named("Section").filter(|s| TEXT.contains(&s.attr("N").unwrap_or(""))) {
            for (n, row) in section.children_named("Row").enumerate() {
                let cells = row
                    .children_named("Cell")
                    .map(|c| {
                        let held = Held { v: c.attr("V").map(str::to_owned), f: c.attr("F").map(str::to_owned), u: c.attr("U").map(str::to_owned) };
                        (c.attr("N").unwrap_or("?").to_owned(), held)
                    })
                    .collect();
                rows.push(TextRow {
                    section: section.attr("N").unwrap_or("").to_owned(),
                    ix: row.attr("IX").map_or(u32::try_from(n).unwrap(), |ix| ix.parse().unwrap()),
                    del: row.attr("Del") == Some("1"),
                    cells,
                });
            }
        }
        let markers = shape.child("Text").map(|text| {
            text.elements()
                .filter_map(|m| {
                    let kind = if m.is("cp") {
                        'c'
                    } else if m.is("pp") {
                        'p'
                    } else {
                        return None;
                    };
                    Some((kind, m.attr("IX").map_or(0, |ix| ix.parse().unwrap())))
                })
                .collect()
        });
        out.push(SavedShape { part: part.to_owned(), id: shape.attr("ID").unwrap_or("?").to_owned(), depth, stands_for, rows, markers });
        if let Some(members) = shape.child("Shapes") {
            walk(members, part, depth + 1, master, masters, out);
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- the checks

/// The rows past the first that lack a cell of `row_cells` which neither they nor the master's row of their `IX` holds: a reader of the file reads such a cell as 0. Named, for a failure.
fn lacking(saved: &Saved) -> Vec<String> {
    let mut out = Vec::new();
    for shape in &saved.shapes {
        let master = saved.master_of(shape);
        for row in shape.rows.iter().filter(|r| r.ix >= 1 && !r.del) {
            let masters_row = master.and_then(|m| m.row(&row.section, row.ix));
            let missing: Vec<&str> = row_cells(&row.section)
                .map(|c| c.name)
                .filter(|name| !row.cells.contains_key(*name) && !masters_row.is_some_and(|m| m.cells.contains_key(*name)))
                .collect();
            if !missing.is_empty() {
                out.push(format!("{} {} IX {} lacks {missing:?}", shape.at(), row.section, row.ix));
            }
        }
    }
    out
}

/// The number a cell holds, if its `V` is one.
fn number(held: &Held) -> Option<f64> {
    held.v.as_deref().and_then(|v| v.trim().parse::<f64>().ok()).filter(|n| n.is_finite())
}

/// Whether a whole number from `from` to `to`.
fn whole_in(held: &Held, from: f64, to: f64) -> bool {
    number(held).is_some_and(|n| n.fract() == 0.0 && (from..=to).contains(&n))
}

/// Whether the cell `name` of a row of `section` holds a value in the range its specification gives (the "At a glance" table and each cell's **Range**). A
/// cell that holds a formula is read by its `V` too (the value a reader takes at load). `faces` are the names of the file's font table.
fn in_range(section: &str, name: &str, held: &Held, faces: &[String]) -> bool {
    let v = held.v.as_deref().unwrap_or("");
    let themed = v == "Themed";
    let face = || v == "0" || v.is_empty() || themed || faces.iter().any(|f| f == v);
    match (section, name) {
        ("Character", "Size") => number(held).is_some_and(|n| n > 0.0 && n <= 1_000.0 / 72.0),
        ("Character", "FontScale") => number(held).is_some_and(|n| n > 0.0 && n <= 6.0),
        ("Character", "Letterspace") => number(held).is_some_and(|n| n.abs() < 1.0),
        ("Character", "Case" | "Pos") => whole_in(held, 0.0, 2.0),
        ("Character", "Style") => themed || whole_in(held, 0.0, 63.0),
        ("Character", "Color") => {
            themed || whole_in(held, 0.0, 23.0) || (v.len() == 7 && v.starts_with('#') && v[1..].bytes().all(|b| b.is_ascii_hexdigit()))
        }
        ("Character", "DblUnderline" | "Overline" | "Strikethru" | "DoubleStrikethrough") => whole_in(held, 0.0, 1.0),
        ("Character", "ComplexScriptSize") => number(held).is_some_and(|n| n != 0.0),
        ("Character", "ColorTrans") => number(held).is_some_and(|n| (0.0..=1.0).contains(&n)),
        ("Character", "Font" | "AsianFont" | "ComplexScriptFont") | ("Paragraph", "BulletFont") => face(),
        // A culture name: letters, digits and hyphens, never a bare number.
        ("Character", "LangID") => {
            !v.is_empty()
                && v.parse::<f64>().is_err()
                && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && v.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        }
        ("Paragraph", "HorzAlign") => whole_in(held, 0.0, 4.0),
        ("Paragraph", "Bullet") => whole_in(held, 0.0, 7.0),
        ("Paragraph", "Flags") => whole_in(held, 0.0, 1.0),
        ("Paragraph", "SpBefore" | "SpAfter") => number(held).is_some_and(|n| n >= 0.0),
        ("Paragraph", "SpLine" | "IndFirst" | "IndLeft" | "IndRight" | "BulletFontSize" | "TextPosAfterBullet") => number(held).is_some(),
        ("Paragraph", "BulletStr" | "LocalizeBulletFont") => true,
        _ => false,
    }
}

/// The cells of text rows out of their range, and the cells no text row of a file holds, named.
fn out_of_range(saved: &Saved) -> Vec<String> {
    let mut out = Vec::new();
    for shape in &saved.shapes {
        for row in shape.rows.iter().filter(|r| !r.del) {
            for (name, held) in &row.cells {
                if !in_range(&row.section, name, held, &saved.faces) {
                    out.push(format!("{} {} IX {} {name} = {held:?}", shape.at(), row.section, row.ix));
                }
            }
        }
    }
    out
}

/// The markers that name a row no reader finds: a `cp` or a `pp` past 0 with no row of its `IX` on the shape or on the master's shape it stands for (row 0 may be the styles').
fn dangling(saved: &Saved) -> Vec<String> {
    let mut out = Vec::new();
    for shape in &saved.shapes {
        let master = saved.master_of(shape);
        // The text a shape shows: its own, or its master's.
        let Some(markers) = shape.markers.as_ref().or_else(|| master.and_then(|m| m.markers.as_ref())) else { continue };
        for (kind, ix) in markers {
            let section = if *kind == 'c' { "Character" } else { "Paragraph" };
            let found = *ix == 0 || shape.row(section, *ix).is_some() || master.is_some_and(|m| m.row(section, *ix).is_some());
            if !found {
                out.push(format!("{} names {section} IX {ix}, which no reader finds", shape.at()));
            }
        }
    }
    out
}

/// All three checks, for a failure that names what is wrong.
fn sound(saved: &Saved, what: &str) {
    let (l, r, d) = (lacking(saved), out_of_range(saved), dangling(saved));
    assert!(l.is_empty(), "{what}: {} rows past the first lack cells: {:?}", l.len(), &l[..l.len().min(8)]);
    assert!(r.is_empty(), "{what}: {} cells out of range: {:?}", r.len(), &r[..r.len().min(8)]);
    assert!(d.is_empty(), "{what}: {} markers name no row: {:?}", d.len(), &d[..d.len().min(8)]);
}

/// The value of a cell of `row_cells` a whole row is given where it says nothing: the value the renderer assumes.
fn assumed(section: &str, name: &str) -> Held {
    let cell: &RootCell = row_cells(section).find(|c| c.name == name).unwrap_or_else(|| panic!("{section} {name} is no cell of the table"));
    Held { v: Some(cell.value.to_owned()), f: None, u: None }
}

// ------------------------------------------------------------------------------------------------------------------------- the drawing

/// The drawing saved as `name` (the file `dir` holds).
fn saved_as(s: &mut Session, dir: &Dir, name: &str) -> Vec<u8> {
    run(s, "file.save_as", json!({"path": dir.path(name)}));
    dir.read(name)
}

/// A blank VSDX file, as a drawing of another program's might be: **its root style states no text cell at all** (an older build's root held `Size` and `Color` alone).
fn bare_vsdx() -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1")).build()
}

// ------------------------------------------------------------------------------------------------------------------------- the org chart

const POSITIONS: [&str; 6] =
    ["orgchart.executive", "orgchart.manager", "orgchart.position", "orgchart.assistant", "orgchart.consultant", "orgchart.vacancy"];

/// What the title row of a position holds: the title's size (8 points) and weight, and every other cell of the table at the value the renderer assumes.
fn title_row() -> BTreeMap<String, Held> {
    let mut want: BTreeMap<String, Held> = row_cells("Character").map(|c| (c.name.to_owned(), assumed("Character", c.name))).collect();
    want.insert("Size".to_owned(), Held { v: Some("0.111111111".to_owned()), f: None, u: Some("IN".to_owned()) });
    want.insert("Style".to_owned(), Held { v: Some("0".to_owned()), f: None, u: None });
    want
}

/// Checks the title row of the master of the position the shape `id` of page 1 is.
fn check_title(saved: &Saved, id: u32, what: &str) {
    let instance = saved.page_shape(id);
    let master = saved.master_of(instance).unwrap_or_else(|| panic!("{what}: shape {id} has no master in the file"));
    let row = master.row("Character", 1).unwrap_or_else(|| panic!("{what}: the master holds no title row"));
    assert_eq!(row.cells, title_row(), "{what}: the title row");
    assert_eq!(row.cells.get("FontScale").and_then(number), Some(1.0), "{what}: the title's letters are of their full width");
    // The name's row leans on the styles, as the rows 0 of files do.
    let name = master.row("Character", 0).expect("the name's row");
    assert_eq!(name.cells.keys().map(String::as_str).collect::<Vec<_>>(), ["Size", "Style"], "{what}: the name's row");
    assert_eq!(master.markers.as_deref(), Some(&[('c', 0), ('c', 1)][..]), "{what}: the name's run and the title's");
    sound(saved, what);
}

/// **The vector, by the model writer**: each position dropped on a new drawing and saved as `.vsdx`. Its master's title row holds the 12 cells, the title's size and weight among them.
#[test]
fn the_title_row_of_each_org_chart_position_is_whole_in_a_new_drawing_saved_as_vsdx() {
    for position in POSITIONS {
        let dir = Dir::new();
        let mut s = pinned();
        let id = id_of(&run(&mut s, "shape.drop", json!({"master": position, "page": 1, "x": 4.0, "y": 6.0})));
        let saved = Saved::read(&saved_as(&mut s, &dir, "chart.vsdx"));
        check_title(&saved, id, position);
    }
}

/// **The vector, by the splice**: each position dropped into a VSDX file that was read, whose root states no text cell, and saved into it.
#[test]
fn the_title_row_of_each_org_chart_position_is_whole_in_a_file_it_is_dropped_into() {
    for position in POSITIONS {
        let dir = Dir::new();
        let mut s = opened(&dir.write("plan.vsdx", &bare_vsdx()));
        let id = id_of(&run(&mut s, "shape.drop", json!({"master": position, "page": 1, "x": 4.0, "y": 6.0})));
        run(&mut s, "file.save", json!({}));
        check_title(&Saved::read(&dir.read("plan.vsdx")), id, position);
    }
}

/// **The ShapeSheet says what is drawn**: the title's `FontScale` reads 1 on the master's sheet and on the shape's (it read 0, the default of a number, where the renderer drew 1), and its `Font`
/// reads 0, which names no face, as it did.
#[test]
fn the_title_rows_cells_read_as_what_is_drawn_on_the_shapesheet() {
    let mut s = pinned();
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "orgchart.manager", "page": 1, "x": 4.0, "y": 6.0})));
    let read = |s: &mut Session, cell: &str| {
        let answer = run(s, "sheet.cell", json!({"id": id, "page": 1, "cell": cell}));
        (answer["value"].as_f64(), answer["origin"].as_str().map(str::to_owned), answer.to_string())
    };
    let (scale, origin, said) = read(&mut s, "Char.FontScale[2]");
    assert_eq!((scale, origin.as_deref()), (Some(1.0), Some("inherited")), "{said}");
    let (size, _, said) = read(&mut s, "Char.Size[2]");
    assert!(size.is_some_and(|n| (n - 8.0 / 72.0).abs() < 1e-9), "{said}");
    let (spacing, origin, said) = read(&mut s, "Char.Letterspace[2]");
    assert_eq!((spacing, origin.as_deref()), (Some(0.0), Some("inherited")), "{said}");
}

// ------------------------------------------------------------------------------------------------------------------------- the language sample

/// The language sample: a text box in Arial at 22 points, a bold run and an italic run, and a run in French (`Char.LangID[2]`) and one in Turkish (`Char.LangID[4]`). → its `ID`.
fn language_box(s: &mut Session) -> u32 {
    let id = id_of(&run(
        s,
        "text.box",
        json!({"page": 1, "x": 4.0, "y": 6.0, "width": 5.0, "height": 1.0, "text": "Hello world Bonjour monde Merhaba dunya"}),
    ));
    run(s, "text.format", json!({"ids": [id], "font": "Arial", "size": 22}));
    run(s, "text.format", json!({"ids": [id], "start": 12, "end": 25, "bold": true}));
    run(s, "text.format", json!({"ids": [id], "start": 26, "end": 39, "italic": true}));
    run(s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "Char.LangID[2]", "formula": "\"fr-FR\""}));
    run(s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "Char.LangID[4]", "formula": "\"tr-TR\""}));
    id
}

/// The runs of the language sample past the first: Arial at 22 points, at full width; the bold one and the italic one.
fn check_language(saved: &Saved, id: u32, what: &str) {
    let shape = saved.page_shape(id);
    let size = 22.0 / 72.0;
    for (ix, style) in [(1, 1.0), (2, 0.0), (3, 2.0)] {
        let row = shape.row("Character", ix).unwrap_or_else(|| panic!("{what}: no run {ix}"));
        let num = |name: &str| row.cells.get(name).and_then(number);
        assert_eq!(row.cells.get("Font").and_then(|h| h.v.as_deref()), Some("Arial"), "{what}: run {ix}");
        assert!(num("Size").is_some_and(|n| (n - size).abs() < 1e-9), "{what}: run {ix} {:?}", row.cells.get("Size"));
        assert_eq!((num("Style"), num("FontScale"), num("Letterspace")), (Some(style), Some(1.0), Some(0.0)), "{what}: run {ix}");
    }
    sound(saved, what);
}

/// **The vector, in a new drawing** (the model writer).
#[test]
fn the_runs_of_a_language_sample_are_whole_in_a_new_drawing() {
    let dir = Dir::new();
    let mut s = pinned();
    let id = language_box(&mut s);
    check_language(&Saved::read(&saved_as(&mut s, &dir, "lang.vsdx")), id, "a new drawing");
}

/// **The vector, in a file whose root states nothing** (the splice): the runs the format made copied the cells their first run resolved to, and the root gave none, so each lacked `FontScale`.
#[test]
fn the_runs_of_a_language_sample_are_whole_in_a_file_whose_root_states_no_text_cell() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("lang.vsdx", &bare_vsdx()));
    let id = language_box(&mut s);
    run(&mut s, "file.save", json!({}));
    check_language(&Saved::read(&dir.read("lang.vsdx")), id, "a file read");
}

// ------------------------------------------------------------------------------------------------------------------------- an `.odg`

/// A flat OpenDocument drawing written by hand from ODF 1.3 (§16.27.28, §16.37 and the text elements of §6): one rectangle whose text is two paragraphs, the first centred with a bold run and a
/// smaller italic run, the second aligned to the end; the shape's style gives the face and 18 points.
const FLAT_TEXT: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    "\n",
    r#"<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.graphics">"#,
    r#"<office:font-face-decls><style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/></office:font-face-decls>"#,
    r#"<office:styles><style:style style:name="standard" style:family="graphic"><style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt"/></style:style></office:styles>"#,
    r#"<office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" "#,
    r#"fo:margin-right="1cm" fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait"/></style:page-layout>"#,
    r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="none" draw:stroke="none"/></style:style>"#,
    r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#,
    r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="end"/></style:style>"#,
    r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#,
    r#"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-style="italic" fo:font-size="12pt"/></style:style></office:automatic-styles>"#,
    r#"<office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles>"#,
    r#"<office:body><office:drawing><draw:page draw:name="page1" draw:master-page-name="Default">"#,
    r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="12cm" svg:height="4cm"><text:p text:style-name="P1">Hello <text:span text:style-name="T1">bold world</text:span> "#,
    r#"and <text:span text:style-name="T2">small italic</text:span></text:p><text:p text:style-name="P2">Second paragraph</text:p></draw:rect>"#,
    r#"</draw:page></office:drawing></office:body></office:document>"#,
    "\n"
);

/// **The vector, an `.odg` saved as VSDX**: the rows the `.odg` reader makes for the runs and the paragraph past the first hold what their styles say and every other cell of the table, so
/// the bold run is 18 points at full width and the second paragraph is at the right with the line spacing of the others.
#[test]
fn the_runs_and_paragraphs_of_an_odg_saved_as_vsdx_are_whole() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("text.fodg", FLAT_TEXT.as_bytes()));
    let saved = Saved::read(&saved_as(&mut s, &dir, "text.vsdx"));
    let shape = saved.shapes.iter().find(|sh| sh.rows.iter().any(|r| r.ix >= 1)).expect("the rectangle's rows");
    let bold = shape.row("Character", 1).expect("the bold run");
    let num = |row: &TextRow, name: &str| row.cells.get(name).and_then(number);
    assert_eq!((num(bold, "Style"), num(bold, "FontScale")), (Some(1.0), Some(1.0)));
    assert!(num(bold, "Size").is_some_and(|n| (n - 0.25).abs() < 1e-9), "{:?}", bold.cells.get("Size"));
    let second = shape.row("Paragraph", 1).expect("the second paragraph");
    assert_eq!((num(second, "HorzAlign"), num(second, "SpLine")), (Some(2.0), Some(-1.2)));
    sound(&saved, "an .odg saved as .vsdx");
    // The drawing becomes what the file reads as, and a save into it with no edit is the same bytes.
    let first = dir.read("text.vsdx");
    run(&mut s, "file.save", json!({}));
    assert_eq!(dir.read("text.vsdx"), first, "a save with no edit after the save across");
}

/// The flat drawing itself is saved as it was: the rows are given to the copy a VSDX writer writes, and not to the drawing.
#[test]
fn an_odg_saved_with_no_edit_after_its_save_as_vsdx_is_the_same_bytes() {
    let dir = Dir::new();
    let path = dir.write("text.fodg", FLAT_TEXT.as_bytes());
    let mut s = opened(&path);
    run(&mut s, "file.save", json!({}));
    assert_eq!(dir.read("text.fodg"), FLAT_TEXT.as_bytes(), "a save with no edit");
}

// ------------------------------------------------------------------------------------------------------------------------- an older build's drawing

/// A drawing **as an older build saved it**: with the root that build made (`EARLIER_ROOT`, which no other program's file holds), a master of a position whose title row holds `Size` and `Style` alone,
/// an instance of it, and a text box whose second run holds its weight alone. (Rows of this shape in a file whose root is another program's are that program's.)
fn older_vsdx() -> Vec<u8> {
    let master = concat!(
        r#"<Shape ID="5" Type="Shape"><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
        r#"<Section N="Character"><Row IX="0"><Cell N="Size" V="0.138888889" U="IN"/><Cell N="Style" V="1"/></Row>"#,
        r#"<Row IX="1"><Cell N="Size" V="0.111111111" U="IN"/><Cell N="Style" V="0"/></Row></Section>"#,
        r#"<Text><cp IX="0"/>Ann Lee"#,
        "\n",
        r#"<cp IX="1"/>Manager</Text></Shape>"#
    );
    let shapes = concat!(
        r#"<Shape ID="1" Type="Shape" Master="1"><Cell N="PinX" V="3" U="IN"/><Cell N="PinY" V="3" U="IN"/></Shape>"#,
        r#"<Shape ID="2" Type="Shape"><Cell N="PinX" V="3" U="IN"/><Cell N="PinY" V="6" U="IN"/><Cell N="Width" V="3" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
        r#"<Section N="Character"><Row IX="0"><Cell N="Size" V="0.25"/></Row><Row IX="1"><Cell N="Style" V="1"/></Row></Section>"#,
        r#"<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="0"/></Row><Row IX="1"><Cell N="HorzAlign" V="2"/></Row></Section>"#,
        r#"<Text><cp IX="0"/><pp IX="0"/>plain <cp IX="1"/>bold"#,
        "\n",
        r#"<pp IX="1"/>right</Text></Shape>"#
    );
    VsdxBuilder::new()
        .styles(EARLIER_ROOT)
        .master(MasterSpec::new(1, "Manager").shapes(master))
        .page(PageSpec::new(1, "Page-1").shapes(shapes))
        .build()
}

/// The bytes of one part of a package.
fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap().part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()
}

/// **A file an older build wrote, saved with no edit, keeps its page and its master as they were, byte for byte**: what the file holds is the file's, and a save gives its rows nothing where
/// nothing was edited (a save into a file another session wrote names DiaCraft in its properties, which is not the rows' concern).
#[test]
fn an_older_file_saved_with_no_edit_keeps_its_page_and_master_parts_byte_for_byte() {
    let dir = Dir::new();
    let bytes = older_vsdx();
    let mut s = opened(&dir.write("old.vsdx", &bytes));
    run(&mut s, "file.save", json!({}));
    let saved = dir.read("old.vsdx");
    for name in [concat!("/vi", "sio", "/pages/page1.xml"), concat!("/vi", "sio", "/masters/master1.xml")] {
        assert_eq!(part(&saved, name), part(&bytes, name), "{name}");
    }
    assert!(!lacking(&Saved::read(&bytes)).is_empty(), "the older file lacks cells, as such files do");
    assert_eq!(lacking(&Saved::read(&saved)), lacking(&Saved::read(&bytes)), "and keeps them as they were");
}

/// **A `.diacraft` drawing of an older build, saved as VSDX** (the model writer): the copy it is given has the rows made whole, the master's title row and the text box's second run and paragraph;
/// the drawing becomes what the file reads as, and a save into it with no edit is the same bytes.
#[test]
fn an_older_drawing_saved_from_diacraft_as_vsdx_has_its_rows_made_whole() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("old.vsdx", &older_vsdx()));
    saved_as(&mut s, &dir, "old.diacraft");
    let mut read = opened(&dir.path("old.diacraft"));
    let saved = Saved::read(&saved_as(&mut read, &dir, "new.vsdx"));
    sound(&saved, "an older drawing from .diacraft");
    let title = saved.master_of(saved.page_shape(1)).and_then(|m| m.row("Character", 1)).expect("the title row");
    assert_eq!(title.cells, title_row());
    let text = saved.page_shape(2);
    let num = |section: &str, ix: u32, name: &str| text.row(section, ix).and_then(|r| r.cells.get(name)).and_then(number);
    assert_eq!(
        (num("Character", 1, "Style"), num("Character", 1, "FontScale"), num("Character", 1, "Size")),
        (Some(1.0), Some(1.0), Some(0.1666667))
    );
    assert_eq!((num("Paragraph", 1, "HorzAlign"), num("Paragraph", 1, "SpLine")), (Some(2.0), Some(-1.2)));
    // Row 0 leans on the styles: it is left as it was.
    assert_eq!(text.row("Character", 0).map(|r| r.cells.len()), Some(1));
    let first = dir.read("new.vsdx");
    run(&mut read, "file.save", json!({}));
    assert_eq!(dir.read("new.vsdx"), first, "a save with no edit after the save from the model");
}

/// The drawing the person has is not changed by the save: the rows are given to the copy the writer writes (and the drawing takes them by becoming what the file reads as).
#[test]
fn an_older_drawing_saved_as_diacraft_keeps_its_rows_as_they_were() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("old.vsdx", &older_vsdx()));
    saved_as(&mut s, &dir, "old.diacraft");
    let mut read = opened(&dir.path("old.diacraft"));
    let before = run(&mut read, "sheet.cell", json!({"id": 2, "page": 1, "cell": "Char.FontScale[2]"}));
    assert_eq!(before["origin"], json!("default"), "the .diacraft file holds the row as the older build made it: {before}");
}

// ------------------------------------------------------------------------------------------------------------------------- the sweeps

/// **Every master of the built-in stencils**: dropped on a drawing and saved, by the model writer and into a file that was read; every row past the first is whole and every cell in range,
/// and each saved as an `.odg` is saved (no cell of a whole row is one the `.odg` writer refuses).
#[test]
fn every_master_of_the_built_in_stencils_has_whole_rows_past_the_first_and_cells_in_range() {
    let dir = Dir::new();
    let mut s = pinned();
    let mut model = opened(&dir.write("plan.vsdx", &bare_vsdx()));
    let mut dropped = 0usize;
    for spec in all_masters() {
        // The Pool and the Swimlane are structures made by code, no instance of a master.
        if spec.makes_structure() {
            continue;
        }
        let at = json!({"master": spec.id, "page": 1, "x": 3.0, "y": 3.0});
        let answer = run(&mut s, "shape.drop", at.clone());
        run(&mut model, "shape.drop", at);
        dropped += usize::from(!answer["master"].is_null());
    }
    assert!(dropped > 100, "the sweep covers the catalogue: {dropped} masters");
    let made = Saved::read(&saved_as(&mut s, &dir, "all.vsdx"));
    let rows_past_first = made.shapes.iter().flat_map(|sh| sh.rows.iter()).filter(|r| r.ix >= 1).count();
    assert!(rows_past_first >= 20, "the catalogue's masters hold rows past the first: {rows_past_first}");
    sound(&made, "the model writer");
    run(&mut model, "file.save", json!({}));
    sound(&Saved::read(&dir.read("plan.vsdx")), "the splice");
    run(&mut s, "file.save_as", json!({"path": dir.path("all.odg")}));
}

/// One `text.format` of each property, each on a text box of its own: `(params, whether it formats a paragraph)`.
fn formats() -> Vec<(Value, bool)> {
    let mut out: Vec<(Value, bool)> = vec![
        (json!({"font": "Arial"}), false),
        (json!({"font": "Liberation Serif"}), false),
        (json!({"size": 20}), false),
        (json!({"bold": true}), false),
        (json!({"italic": true}), false),
        (json!({"underline": true}), false),
        (json!({"strike": true}), false),
        (json!({"color": "#336699"}), false),
        (json!({"color": "theme:accent2:40"}), false),
        (json!({"color": "default"}), false),
        (json!({"bold": true, "italic": true, "underline": true, "size": 9, "font": "Arial"}), false),
        (json!({"indent": "increase"}), true),
        (json!({"direction": "rtl"}), true),
    ];
    for case in ["normal", "all_caps", "initial_caps", "small_caps"] {
        out.push((json!({"case": case}), false));
    }
    for align in ["left", "center", "right", "justify", "distributed"] {
        out.push((json!({"align": align}), true));
    }
    for bullet in 0..=7 {
        out.push((json!({"bullet": bullet}), true));
    }
    out
}

/// Each format on a text box of its own, on a range inside its text (the middle of a run, and the middle paragraph of three), so that the runs and paragraphs are split. → how many were made.
fn format_each(s: &mut Session) -> usize {
    let mut made = 0usize;
    for (n, (params, paragraph)) in formats().into_iter().enumerate() {
        let y = 1.0 + 0.4 * n as f64;
        let id = id_of(&run(
            s,
            "text.box",
            json!({"page": 1, "x": 4.0, "y": y, "width": 5.0, "height": 0.3, "text": "first line\nsecond line\nthird line"}),
        ));
        let mut params = params;
        let (start, end) = if paragraph { (14, 17) } else { (2, 6) };
        params["ids"] = json!([id]);
        params["start"] = json!(start);
        params["end"] = json!(end);
        let answer = run(s, "text.format", params);
        made += usize::from(answer["formatted"] == json!(1));
    }
    made
}

/// **Text formatted by `text.format` with each property**, in a new drawing and in a file whose root states nothing: every row the split makes past the first is whole and in range, by the model
/// writer, by the splice and after a trip through `.diacraft`, and the drawing saved as an `.odg` is saved.
#[test]
fn text_formatted_with_each_property_has_whole_rows_past_the_first_and_cells_in_range() {
    let dir = Dir::new();
    let mut s = pinned();
    assert!(format_each(&mut s) >= 25, "each format changes its box");
    let made = Saved::read(&saved_as(&mut s, &dir, "formats.vsdx"));
    let rows_past_first = made.shapes.iter().flat_map(|sh| sh.rows.iter()).filter(|r| r.ix >= 1).count();
    assert!(rows_past_first >= 50, "the splits make rows past the first: {rows_past_first}");
    sound(&made, "a new drawing");
    run(&mut s, "file.save_as", json!({"path": dir.path("formats.odg")}));
    let mut read = opened(&dir.write("bare.vsdx", &bare_vsdx()));
    format_each(&mut read);
    run(&mut read, "file.save", json!({}));
    sound(&Saved::read(&dir.read("bare.vsdx")), "a file whose root states nothing");
    // The drawing as a `.diacraft` file, saved as VSDX: the model writer of a drawing that came from a file.
    saved_as(&mut read, &dir, "bare.diacraft");
    let mut again = opened(&dir.path("bare.diacraft"));
    sound(&Saved::read(&saved_as(&mut again, &dir, "again.vsdx")), "a .diacraft trip");
}

/// **A file saved after the fix and read again, saved with no edit, is the same bytes** (the org chart and the language sample).
#[test]
fn a_file_with_whole_rows_read_and_saved_with_no_edit_is_the_same_bytes() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "shape.drop", json!({"master": "orgchart.executive", "page": 1, "x": 4.0, "y": 8.0}));
    language_box(&mut s);
    let first = saved_as(&mut s, &dir, "a.vsdx");
    let mut read = opened(&dir.path("a.vsdx"));
    run(&mut read, "file.save", json!({}));
    assert_eq!(dir.read("a.vsdx"), first);
}

// ------------------------------------------------------------------------------------------------------------------------- whose rows they are

/// A root style as DiaCraft made it before it stated the text cells (`diacraft_doc::root_text::is_an_earlier_root`): `No Style`, no `Paragraph` section, and a `Character` row 0 of a size and a colour.
const EARLIER_ROOT: &str = concat!(
    r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/>"#,
    r#"<Section N="Character"><Row IX="0"><Cell N="Size" V="0.1666666666666667"/><Cell N="Color" V="0"/></Row></Section></StyleSheet>"#
);

/// The `docProps/app.xml` of a program that is not this one.
const OTHER_PROGRAM: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">"#,
    r#"<Application>Quill Draw</Application><AppVersion>3.1</AppVersion></Properties>"#
);

/// **A VSDX file of another program's own**: a rectangle with no master. Its `Character` section holds row `IX` 0 whole and row `IX` 1 with a size and a style alone, and its `Paragraph` section
/// row 1 with an indent alone; the text is two runs and two paragraphs, "Alder " and "Birch".
fn foreign_vsdx(root: &str) -> Vec<u8> {
    let whole: String = row_cells("Character")
        .map(|c| format!(r#"<Cell N="{}" V="{}"/>"#, c.name, c.value))
        .chain([r#"<Cell N="Font" V="0"/>"#.to_owned()])
        .collect();
    let shapes = format!(
        concat!(
            r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="3" U="IN"/><Cell N="PinY" V="3" U="IN"/><Cell N="Width" V="3" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
            r#"<Section N="Character"><Row IX="0">{whole}</Row><Row IX="1"><Cell N="Size" V="0.25" U="PT"/><Cell N="Style" V="1"/></Row></Section>"#,
            r#"<Section N="Paragraph"><Row IX="0"><Cell N="IndLeft" V="0.1"/></Row><Row IX="1"><Cell N="IndLeft" V="0.4" U="IN"/></Row></Section>"#,
            r#"<Text><cp IX="0"/><pp IX="0"/>Alder <cp IX="1"/>Birch"#,
            "\n",
            r#"<pp IX="1"/>Second</Text></Shape>"#
        ),
        whole = whole
    );
    VsdxBuilder::new().styles(root).app(OTHER_PROGRAM).page(PageSpec::new(1, "Page-1").shapes(&shapes)).build()
}

/// The cells of row `ix` of `section` on the shape `id` of a saved package's first page, as `(name, V)` in name order.
fn row_of(saved: &Saved, id: u32, section: &str, ix: u32) -> Vec<(String, Option<String>)> {
    let row = saved.page_shape(id).row(section, ix).unwrap_or_else(|| panic!("no {section} IX {ix} on shape {id}"));
    row.cells.iter().map(|(name, held)| (name.clone(), held.v.clone())).collect()
}

/// **G2, the vector**: a VSDX file of another program's, converted to `.diacraft` and that to VSDX, holds the rows it held: row `IX` 1 keeps its size and its style, and no cell is made up for it
/// (the splice writes the same). A reader of the file takes a cell its row lacks from the master's row of the same `IX` and from nowhere else: it is the file's own to say, and a save through
/// `.diacraft` does not say it for the program that wrote it.
#[test]
fn a_vsdx_files_own_rows_are_written_as_read_on_every_route() {
    for (what, root) in [("a root that states nothing", ROOT_STYLE), ("the root of another program", FOREIGN_ROOT)] {
        let dir = Dir::new();
        let source = foreign_vsdx(root);
        let want_char: Vec<(String, Option<String>)> = vec![("Size".into(), Some("0.25".into())), ("Style".into(), Some("1".into()))];
        let want_para: Vec<(String, Option<String>)> = vec![("IndLeft".into(), Some("0.4".into()))];
        // The control: the splice, a save into the file.
        let mut spliced = opened(&dir.write("spliced.vsdx", &source));
        run(&mut spliced, "file.save", json!({}));
        let saved = Saved::read(&dir.read("spliced.vsdx"));
        assert_eq!(row_of(&saved, 1, "Character", 1), want_char, "{what}: the splice");
        // A copy by the model writer (Save As to another VSDX), and the same through a `.diacraft` file.
        let mut copy = opened(&dir.write("source.vsdx", &source));
        let copied = Saved::read(&saved_as(&mut copy, &dir, "copy.vsdx"));
        saved_as(&mut copy, &dir, "source.diacraft");
        let mut native = opened(&dir.path("source.diacraft"));
        let through = Saved::read(&saved_as(&mut native, &dir, "through.vsdx"));
        for (route, saved) in [("a copy", &copied), ("through .diacraft", &through)] {
            assert_eq!(row_of(saved, 1, "Character", 1), want_char, "{what}: {route}, the title row");
            assert_eq!(row_of(saved, 1, "Paragraph", 1), want_para, "{what}: {route}, the second paragraph");
            assert_eq!(row_of(saved, 1, "Character", 0).len(), row_cells("Character").count() + 1, "{what}: {route}, row 0 as it was");
            assert_eq!(
                row_of(saved, 1, "Paragraph", 0),
                vec![("IndLeft".to_owned(), Some("0.1".to_owned()))],
                "{what}: {route}, row 0 of the paragraphs"
            );
        }
        // And a save with no edit of what it wrote is the same bytes.
        let first = dir.read("through.vsdx");
        run(&mut native, "file.save", json!({}));
        assert_eq!(dir.read("through.vsdx"), first, "{what}: a save with no edit");
    }
}

/// The root of another program: the whole of the cells DiaCraft's own states, and a face, a language and a colour table row besides, as every root of a program's files does.
const FOREIGN_ROOT: &str = concat!(
    r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/>"#,
    r#"<Section N="Character"><Row IX="0"><Cell N="Font" V="0"/><Cell N="Color" V="0"/><Cell N="Style" V="0"/><Cell N="Size" V="0.1666666666666667"/></Row></Section>"#,
    r#"<Section N="Paragraph"><Row IX="0"><Cell N="HorzAlign" V="1"/><Cell N="SpLine" V="-1.2"/></Row></Section></StyleSheet>"#
);

/// A drawing **as an older build saved it** has the root that build made, which no other program's file holds (`is_an_earlier_root`), so its rows are DiaCraft's to state through the model writer.
#[test]
fn the_rows_of_a_file_with_the_root_of_an_older_build_are_made_whole_through_diacraft() {
    let dir = Dir::new();
    let source = foreign_vsdx(EARLIER_ROOT);
    let mut s = opened(&dir.write("old.vsdx", &source));
    saved_as(&mut s, &dir, "old.diacraft");
    let mut native = opened(&dir.path("old.diacraft"));
    let saved = Saved::read(&saved_as(&mut native, &dir, "new.vsdx"));
    sound(&saved, "an older build's file through .diacraft");
    let row = row_of(&saved, 1, "Character", 1);
    assert_eq!(row.len(), row_cells("Character").count(), "the row is whole: {row:?}");
    assert!(row.iter().any(|(n, v)| n == "Size" && v.as_deref() == Some("0.25")), "{row:?}");
}

// ------------------------------------------------------------------------------------------------------------------------- an `.odg` across a VSDX and back

/// A flat OpenDocument drawing written by hand from ODF 1.3: one rectangle whose text is two paragraphs, "Alder" (paragraph style `P1`, a margin of 0.5 cm) and "Birch" (`P2`, 1 cm, in a span
/// of the text style `T1`: another face, 14 points, where the shape's style gives Liberation Sans at 18). **No `fo:text-align` is stated anywhere**, so both paragraphs are left-aligned, the
/// ODF default.
fn flat_unaligned() -> String {
    concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        "\n",
        r#"<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
        r#"office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.graphics">"#,
        r#"<office:font-face-decls><style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/>"#,
        r#"<style:font-face style:name="Liberation Serif" svg:font-family="'Liberation Serif'"/></office:font-face-decls>"#,
        r#"<office:styles><style:style style:name="standard" style:family="graphic"><style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt"/></style:style></office:styles>"#,
        r#"<office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" "#,
        r#"fo:margin-right="1cm" fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait"/></style:page-layout>"#,
        r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="none" draw:stroke="none"/></style:style>"#,
        r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:margin-left="0.5cm"/></style:style>"#,
        r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:margin-left="1cm"/></style:style>"#,
        r#"<style:style style:name="T1" style:family="text"><style:text-properties style:font-name="Liberation Serif" fo:font-size="14pt"/></style:style></office:automatic-styles>"#,
        r#"<office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles>"#,
        r#"<office:body><office:drawing><draw:page draw:name="page1" draw:master-page-name="Default">"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="12cm" svg:height="4cm"><text:p text:style-name="P1">Alder</text:p>"#,
        r#"<text:p text:style-name="P2"><text:span text:style-name="T1">Birch</text:span></text:p></draw:rect>"#,
        r#"</draw:page></office:drawing></office:body></office:document>"#,
        "\n"
    )
    .to_owned()
}

/// What an `.odg`'s `content.xml` says of one paragraph: the name and the properties of the automatic style it names, and each of its spans the same way.
#[derive(Debug, PartialEq)]
struct Said {
    text: String,
    paragraph: BTreeMap<String, String>,
    /// Each span: its text, the name of its style and the properties of that style.
    spans: Vec<(String, Option<String>, BTreeMap<String, String>)>,
}

/// The elements under `el` whose local name is `local`, in document order.
fn below<'a>(el: &'a El, local: &str, out: &mut Vec<&'a El>) {
    for child in el.elements() {
        if child.is(local) {
            out.push(child);
        }
        below(child, local, out);
    }
}

/// The attributes of the properties element `local` (`paragraph-properties`, `text-properties`) of the automatic style `name`.
fn style_props(automatic: &El, name: &str, local: &str) -> BTreeMap<String, String> {
    let mut styles = Vec::new();
    below(automatic, "style", &mut styles);
    let Some(style) = styles.into_iter().find(|s| s.attr("style:name") == Some(name)) else {
        return BTreeMap::new();
    };
    style.child(local).map(|p| p.attrs.iter().cloned().collect()).unwrap_or_default()
}

/// The paragraphs of an `.odg`'s `content.xml`, with what their styles say.
fn said_by(content: &str) -> Vec<Said> {
    let root = xml::parse(content.as_bytes()).unwrap();
    let automatic = root.child("automatic-styles").unwrap();
    let mut paragraphs = Vec::new();
    below(&root, "p", &mut paragraphs);
    paragraphs
        .into_iter()
        .map(|p| {
            let style = p.attr("text:style-name").map(str::to_owned);
            let mut spans = Vec::new();
            let mut text = p.text();
            for span in p.children_named("span") {
                let name = span.attr("text:style-name").map(str::to_owned);
                let props = name.as_deref().map(|n| style_props(automatic, n, "text-properties")).unwrap_or_default();
                text.push_str(&span.text());
                spans.push((span.text(), name, props));
            }
            let paragraph = style.as_deref().map(|n| style_props(automatic, n, "paragraph-properties")).unwrap_or_default();
            Said { text, paragraph, spans }
        })
        .collect()
}

/// **G3, the vector**: an `.odg` rectangle of two paragraphs that state no alignment, converted to VSDX and that to `.odg`. The VSDX's `Paragraph` row `IX` 1 holds its indent and no alignment other
/// than 0 (left, which is what the `.odg` says of a paragraph that states none, and what a program that reads the file takes of a cell its row lacks), and the `.odg` it comes back as leaves both
/// paragraphs left-aligned, in the styles and faces and sizes the file stated: the held styles of the paragraphs and spans still stand beside the rows.
#[test]
fn an_odgs_paragraphs_that_state_no_alignment_stay_left_across_a_vsdx_and_back() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("src.fodg", flat_unaligned().as_bytes()));
    let mid = Saved::read(&saved_as(&mut s, &dir, "mid.vsdx"));
    // The paragraph row the second paragraph is on: its indent, and no alignment but 0.
    let shape = mid.shapes.iter().find(|sh| sh.rows.iter().any(|r| r.section == "Paragraph" && r.ix == 1)).expect("the rectangle's paragraph rows");
    let para = shape.row("Paragraph", 1).unwrap();
    assert!(para.cells.get("IndLeft").and_then(number).is_some_and(|n| (n - 0.3937).abs() < 1e-3), "{:?}", para.cells.get("IndLeft"));
    assert!(
        para.cells.get("HorzAlign").is_none_or(|c| number(c) == Some(0.0)),
        "the second paragraph's alignment: {:?}",
        para.cells.get("HorzAlign")
    );
    // The run row stays whole, in the face and size the span stated.
    let run_row = shape.row("Character", 1).unwrap();
    assert_eq!(run_row.cells.get("FontScale").and_then(number), Some(1.0));
    assert_eq!(run_row.cells.get("Font").and_then(|c| c.v.as_deref()), Some("Liberation Serif"));
    sound(&mid, "an .odg with no alignment, as VSDX");
    // And back to an `.odg`: the paragraphs the file had, with the styles it had.
    let mut again = opened(&dir.path("mid.vsdx"));
    let back = saved_as(&mut again, &dir, "back.odg");
    let said = said_by(&crate::odg_build::part_of(&back, "content.xml"));
    assert_eq!(said.len(), 2, "{said:?}");
    for paragraph in &said {
        let align = paragraph.paragraph.get("fo:text-align").map(String::as_str);
        assert!(matches!(align, None | Some("left" | "start")), "{} is not left-aligned: {paragraph:?}", paragraph.text);
        assert!(!paragraph.paragraph.contains_key("fo:line-height"), "{} gained a line height: {paragraph:?}", paragraph.text);
    }
    assert_eq!(said[0].paragraph.get("fo:margin-left").map(String::as_str), Some("0.5cm"), "{:?}", said[0]);
    assert_eq!(said[1].paragraph.get("fo:margin-left").map(String::as_str), Some("1cm"), "{:?}", said[1]);
    let (text, name, props) = said[1].spans.first().expect("Birch's span");
    assert_eq!(text, "Birch");
    assert_eq!(name.as_deref(), Some("T1"), "the held style of the span is the one it names: {props:?}");
    assert_eq!(props.get("style:font-name").map(String::as_str), Some("Liberation Serif"), "{props:?}");
    assert_eq!(props.get("fo:font-size").map(String::as_str), Some("14pt"), "{props:?}");
    // What the file did not state is not stated: no colour, no scale, no transform on the span.
    for stray in ["fo:color", "style:text-scale", "fo:text-transform", "fo:letter-spacing"] {
        assert!(!props.contains_key(stray), "{stray} was written: {props:?}");
    }
}

/// What the file states stays what it says across the VSDX and back (the alignments, the weight, the italic and the size of the spans of the flat drawing above this one): the rows are completed
/// beside the held styles, not over them.
#[test]
fn an_odgs_stated_alignments_and_sizes_stay_across_a_vsdx_and_back() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("text.fodg", FLAT_TEXT.as_bytes()));
    saved_as(&mut s, &dir, "mid.vsdx");
    let mut again = opened(&dir.path("mid.vsdx"));
    let back = saved_as(&mut again, &dir, "back.odg");
    let said = said_by(&crate::odg_build::part_of(&back, "content.xml"));
    assert_eq!(said.len(), 2, "{said:?}");
    assert_eq!(said[0].paragraph.get("fo:text-align").map(String::as_str), Some("center"), "{:?}", said[0]);
    assert_eq!(said[1].paragraph.get("fo:text-align").map(String::as_str), Some("end"), "{:?}", said[1]);
    let spans = &said[0].spans;
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert_eq!(spans[0].2.get("fo:font-weight").map(String::as_str), Some("bold"), "{:?}", spans[0]);
    assert!(!spans[0].2.contains_key("fo:font-size"), "the bold run states no size: {:?}", spans[0]);
    assert_eq!(spans[1].2.get("fo:font-size").map(String::as_str), Some("12pt"), "{:?}", spans[1]);
    assert_eq!(spans[1].2.get("fo:font-style").map(String::as_str), Some("italic"), "{:?}", spans[1]);
}

fn flat_rich() -> String {
    concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        "\n",
        r#"<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
        r#"office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.graphics">"#,
        r#"<office:font-face-decls><style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/>"#,
        r#"<style:font-face style:name="Liberation Serif" svg:font-family="'Liberation Serif'"/><style:font-face style:name="DejaVu Sans" svg:font-family="'DejaVu Sans'"/></office:font-face-decls>"#,
        r##"<office:styles><style:style style:name="standard" style:family="graphic"><style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt" fo:color="#336699"/></style:style></office:styles>"##,
        r#"<office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" "#,
        r#"fo:margin-right="1cm" fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait"/></style:page-layout>"#,
        r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="none" draw:stroke="none"/></style:style>"#,
        r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:margin-left="0.5cm" fo:line-height="150%"/></style:style>"#,
        r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="justify" fo:margin-top="0.2cm"/><style:text-properties fo:font-size="12pt" fo:font-weight="bold"/></style:style>"#,
        r#"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="start"/></style:style>"#,
        r##"<style:style style:name="T1" style:family="text"><style:text-properties style:font-name="DejaVu Sans" fo:font-size="80%" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="#ff0000"/></style:style>"##,
        r##"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic" fo:color="#aa0000" style:text-line-through-style="solid"/></style:style>"##,
        r#"<style:style style:name="T3" style:family="text"><style:text-properties style:text-position="super 58%" fo:text-transform="lowercase"/></style:style></office:automatic-styles>"#,
        r#"<office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles>"#,
        r#"<office:body><office:drawing><draw:page draw:name="page1" draw:master-page-name="Default">"#,
        r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="12cm" svg:height="8cm">"#,
        r#"<text:p text:style-name="P1">Alder <text:span text:style-name="T1">pine</text:span> and <text:span text:style-name="T2">oak</text:span></text:p>"#,
        r#"<text:p text:style-name="P2">Birch <text:span text:style-name="T3">ELM</text:span> tail</text:p>"#,
        r#"<text:p text:style-name="P3">Plain <text:span text:style-name="T2">red</text:span></text:p>"#,
        r#"<text:p>Unstyled</text:p></draw:rect>"#,
        r#"</draw:page></office:drawing></office:body></office:document>"#,
        "\n"
    )
    .to_owned()
}

/// **What the `.odg` says of its text is what it says after a trip through VSDX**: a rectangle of four paragraphs and five spans (a relative size and an underline in another face, a bold italic
/// run with a line through it in its own colour, a superscript in lower case, a justified paragraph with a weight of its own, an alignment spelled `start`, a line spacing, a margin and a
/// paragraph that names no style) is written to an `.odg` as it is, and again after a save as VSDX and a read of that: every paragraph and every span holds the same properties in both, so no row
/// completed for the VSDX file is said as a statement the `.odg` never made.
#[test]
fn an_odgs_text_says_the_same_after_a_trip_through_vsdx_as_when_saved_directly() {
    let dir = Dir::new();
    let mut direct = opened(&dir.write("src.fodg", flat_rich().as_bytes()));
    let kept = saved_as(&mut direct, &dir, "direct.odg");
    let mut across = opened(&dir.write("src2.fodg", flat_rich().as_bytes()));
    let mid = Saved::read(&saved_as(&mut across, &dir, "mid.vsdx"));
    // (A paragraph that names no style has a marker and no row, as the `.odg` reader makes it: `sound` would name that marker. The rows there are whole.)
    assert!(lacking(&mid).is_empty(), "{:?}", lacking(&mid));
    assert!(out_of_range(&mid).is_empty(), "{:?}", out_of_range(&mid));
    let mut again = opened(&dir.path("mid.vsdx"));
    let back = saved_as(&mut again, &dir, "back.odg");
    let (a, b) = (said_by(&crate::odg_build::part_of(&kept, "content.xml")), said_by(&crate::odg_build::part_of(&back, "content.xml")));
    assert_eq!(a.len(), 4, "{a:?}");
    for (kept, back) in a.iter().zip(&b) {
        assert_eq!(kept, back, "the paragraph {:?}", kept.text);
    }
    assert_eq!(a.len(), b.len());
    // The same through a `.diacraft` file between the two.
    let mut native = opened(&dir.write("src3.fodg", flat_rich().as_bytes()));
    saved_as(&mut native, &dir, "rich.diacraft");
    let mut from_native = opened(&dir.path("rich.diacraft"));
    saved_as(&mut from_native, &dir, "mid2.vsdx");
    let mut again = opened(&dir.path("mid2.vsdx"));
    let via = said_by(&crate::odg_build::part_of(&saved_as(&mut again, &dir, "back2.odg"), "content.xml"));
    for (kept, back) in a.iter().zip(&via) {
        assert_eq!(kept, back, "through .diacraft, the paragraph {:?}", kept.text);
    }
}

// ------------------------------------------------------------------------------------------------------------------------- a list's paragraphs across a VSDX and back

/// A flat OpenDocument drawing written by hand from ODF 1.3 (§16.27.28, §6.3 and §16.30): a text box in a frame holding a paragraph (`P1`) and a bulleted list nested two levels (the list style `L1`,
/// bullets at levels 1 and 2), whose two items are paragraphs of the style `P2` (a margin of 0.6 cm and a text indent of -0.6 cm, and what `p2` adds), each with a span of 12 points and one in bold.
/// `p1` is what the first paragraph's style adds.
fn flat_list(p1: &str, p2: &str, list_style: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            "\n",
            r#"<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
            r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
            r#"office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.graphics">"#,
            r#"<office:font-face-decls><style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/></office:font-face-decls>"#,
            r#"<office:styles><style:style style:name="standard" style:family="graphic"><style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt"/></style:style></office:styles>"#,
            r#"<office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" "#,
            r#"fo:margin-right="1cm" fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait"/></style:page-layout>"#,
            r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="none" draw:stroke="none"/></style:style>"#,
            r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties{p1}/></style:style>"#,
            r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:margin-left="0.6cm" fo:text-indent="-0.6cm"{p2}/></style:style>"#,
            r#"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-size="12pt"/></style:style>"#,
            r#"<style:style style:name="T3" style:family="text"><style:text-properties fo:font-size="12pt" fo:font-weight="bold"/></style:style>"#,
            r#"<text:list-style style:name="L1">{list_style}</text:list-style></office:automatic-styles>"#,
            r#"<office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles>"#,
            r#"<office:body><office:drawing><draw:page draw:name="page1" draw:master-page-name="Default">"#,
            r#"<draw:frame draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="12cm" svg:height="8cm"><draw:text-box>"#,
            r#"<text:p text:style-name="P1"><text:span text:style-name="T2">Alder first line</text:span></text:p>"#,
            r#"<text:list text:style-name="L1"><text:list-item><text:list><text:list-item>"#,
            r#"<text:p text:style-name="P2"><text:span text:style-name="T3">Birch</text:span><text:span text:style-name="T2"> second line</text:span></text:p></text:list-item>"#,
            r#"<text:list-item><text:p text:style-name="P2"><text:span text:style-name="T2">Cedar third line</text:span></text:p></text:list-item>"#,
            r#"</text:list></text:list-item></text:list></draw:text-box></draw:frame>"#,
            r#"</draw:page></office:drawing></office:body></office:document>"#,
            "\n"
        ),
        p1 = p1,
        p2 = p2,
        list_style = list_style
    )
}

/// The levels of the list style `L1` the vector uses: bullets at levels 1 and 2.
const BULLETS: &str = concat!(
    r#"<text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"><style:list-level-properties text:space-before="0.6cm" text:min-label-width="0.6cm"/></text:list-level-style-bullet>"#,
    r#"<text:list-level-style-bullet text:level="2" text:bullet-char="&#8226;"><style:list-level-properties text:space-before="1.2cm" text:min-label-width="0.6cm"/></text:list-level-style-bullet>"#
);

/// Bullets of a character of the file's own, in a face and at a size of their own.
const DASHES: &str = concat!(
    r#"<text:list-level-style-bullet text:level="1" text:bullet-char="-"><style:list-level-properties text:space-before="0.6cm" text:min-label-width="0.6cm"/>"#,
    r#"<style:text-properties style:font-name="Liberation Sans" fo:font-size="14pt"/></text:list-level-style-bullet>"#,
    r#"<text:list-level-style-bullet text:level="2" text:bullet-char="-"><style:list-level-properties text:space-before="1.2cm" text:min-label-width="0.6cm"/>"#,
    r#"<style:text-properties style:font-name="Liberation Sans" fo:font-size="14pt"/></text:list-level-style-bullet>"#
);

/// A numbered list: the writer keeps it, and its paragraphs have no bullet cell.
const NUMBERS: &str = concat!(
    r#"<text:list-level-style-number text:level="1" style:num-suffix="." style:num-format="1"><style:list-level-properties text:space-before="0.6cm" text:min-label-width="0.6cm"/></text:list-level-style-number>"#,
    r#"<text:list-level-style-number text:level="2" style:num-suffix="." style:num-format="1"><style:list-level-properties text:space-before="1.2cm" text:min-label-width="0.6cm"/></text:list-level-style-number>"#
);

/// **G3b, the vector**: the list's paragraphs keep what their styles stated through a VSDX and back: the space above (`M`, `MJ`), the space below (`MB`), the justification (`MJ`, `FJ`). The VSDX row they
/// stand on holds the bullet and the cells a list's paragraph has; a completed row must still be the row their held styles were read with, or the `.odg` writer writes the row's cells and drops
/// what the file stated for them.
#[test]
fn the_paragraphs_of_a_list_keep_their_margins_and_justify_across_a_vsdx_and_back() {
    let variants = [
        ("M", "", r#" fo:margin-top="0.3cm""#),
        ("MB", "", r#" fo:margin-bottom="0.3cm""#),
        ("MJ", r#" fo:text-align="justify""#, r#" fo:text-align="justify" fo:margin-top="0.3cm""#),
        ("FJ", r#" fo:text-align="justify""#, r#" fo:text-align="justify" fo:margin-bottom="0.1cm""#),
    ];
    let mut differ = Vec::new();
    for (what, p1, p2) in variants {
        let dir = Dir::new();
        let source = flat_list(p1, p2, BULLETS);
        let direct = {
            let mut s = opened(&dir.write("src.fodg", source.as_bytes()));
            said_by(&crate::odg_build::part_of(&saved_as(&mut s, &dir, "direct.odg"), "content.xml"))
        };
        let mut across = opened(&dir.write("src2.fodg", source.as_bytes()));
        let mid = Saved::read(&saved_as(&mut across, &dir, "mid.vsdx"));
        assert!(lacking(&mid).is_empty(), "{what}: the rows are whole: {:?}", lacking(&mid));
        let mut again = opened(&dir.path("mid.vsdx"));
        let back = said_by(&crate::odg_build::part_of(&saved_as(&mut again, &dir, "back.odg"), "content.xml"));
        assert_eq!(direct.len(), 3, "{what}: {direct:?}");
        assert_eq!(back.len(), direct.len(), "{what}: {back:?}");
        for (kept, back) in direct.iter().zip(&back) {
            if kept != back {
                differ.push(format!("{what}: the paragraph {:?} was {kept:?} and is {back:?}", kept.text));
            }
        }
    }
    assert!(differ.is_empty(), "{} paragraphs differ after the trip: {differ:#?}", differ.len());
}

/// The same for a bullet of a character of the file's own with a face and a size, and for a numbered list (which the writer keeps, and whose paragraphs hold no bullet cell): what the `.odg` says of
/// its list paragraphs is what it says after a trip through VSDX.
#[test]
fn the_paragraphs_of_other_lists_keep_what_their_styles_stated_across_a_vsdx_and_back() {
    for (what, list) in [("dashes", DASHES), ("numbers", NUMBERS)] {
        let dir = Dir::new();
        let source = flat_list(r#" fo:text-align="justify""#, r#" fo:text-align="justify" fo:margin-top="0.3cm" fo:margin-bottom="0.1cm""#, list);
        let mut direct = opened(&dir.write("src.fodg", source.as_bytes()));
        let kept = said_by(&crate::odg_build::part_of(&saved_as(&mut direct, &dir, "direct.odg"), "content.xml"));
        let mut across = opened(&dir.write("src2.fodg", source.as_bytes()));
        let mid = Saved::read(&saved_as(&mut across, &dir, "mid.vsdx"));
        assert!(lacking(&mid).is_empty(), "{what}: {:?}", lacking(&mid));
        let mut again = opened(&dir.path("mid.vsdx"));
        let back = said_by(&crate::odg_build::part_of(&saved_as(&mut again, &dir, "back.odg"), "content.xml"));
        assert_eq!(kept.len(), 3, "{what}: {kept:?}");
        assert_eq!(kept.len(), back.len(), "{what}: {back:?}");
        for (kept, back) in kept.iter().zip(&back) {
            assert_eq!(kept, back, "{what}: the paragraph {:?}", kept.text);
        }
    }
}

/// **A VSDX file of another program's own, with a bullet at the default size** (`BulletFontSize` -1, "same size": the value the root of every file of the census holds), saved as `.odg`: its paragraph
/// keeps its space above and is an item of a list. The `.odg` writer took the -1 for a size it could not write and left the paragraph with no style at all.
#[test]
fn a_vsdx_files_bullet_paragraph_at_the_default_bullet_size_is_written_to_an_odg_with_its_spacing() {
    let shapes = concat!(
        r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="3" U="IN"/><Cell N="PinY" V="3" U="IN"/><Cell N="Width" V="3" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
        r#"<Section N="Paragraph"><Row IX="0"><Cell N="IndLeft" V="0"/></Row><Row IX="1"><Cell N="Bullet" V="1"/><Cell N="BulletFont" V="0"/><Cell N="BulletFontSize" V="-1"/>"#,
        r#"<Cell N="BulletStr" V=""/><Cell N="HorzAlign" V="3"/><Cell N="IndFirst" V="-0.2"/><Cell N="IndLeft" V="0.2"/><Cell N="SpBefore" V="0.1"/></Row></Section>"#,
        r#"<Text><pp IX="0"/>Alder"#,
        "\n",
        r#"<pp IX="1"/>Birch</Text></Shape>"#
    );
    let dir = Dir::new();
    let bytes = VsdxBuilder::new()
        .app(OTHER_PROGRAM)
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(shapes))
        .build();
    let mut s = opened(&dir.write("bullets.vsdx", &bytes));
    let odg = saved_as(&mut s, &dir, "bullets.odg");
    let content = crate::odg_build::part_of(&odg, "content.xml");
    assert!(content.contains("<text:list"), "the paragraph is an item of a list: {content}");
    let said = said_by(&content);
    let birch = said.iter().find(|p| p.text == "Birch").expect("the bulleted paragraph");
    assert_eq!(birch.paragraph.get("fo:margin-top").map(String::as_str), Some("0.254cm"), "{birch:?}");
    assert_eq!(birch.paragraph.get("fo:text-align").map(String::as_str), Some("justify"), "{birch:?}");
}
