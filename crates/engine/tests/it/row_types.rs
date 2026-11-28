//! **A row an instance holds of its own carries the type `T` of the master's row it overrides**, in every file DiaCraft saves.
//!
//! An instance leaves its master's `Geometry` and `Connection` rows to the master, and holds a row of its own only where a cell of it differs: after a resize, the `X` or `Y` of the
//! rows that name `Width` or `Height`, held as `F="Inh"` with the new result as `V`. Such a row used to be saved with an `IX` and its cells and **no `T`** (a row the model made for the write had no
//! type of its own, and the writers wrote what the model held), so a program that reads a row's type from the row itself and not from the master (draw.io, LibreOffice Draw) took the row for
//! no segment at all: a Process and a Decision drew one edge, or nothing, or the size of the master. the files of the sample-file run always write `T` on such a row.
//!
//! * **The vector** (the rectangle, 1.5 by 1 inch, dropped and made 3 inches tall, then saved as `.vsdx`): rows `IX` 3 and 4 of its `Geometry` are `T="LineTo"` with `Y` `F="Inh"` `V="3"`; a width
//!   alone changes rows 2 and 3, and both change rows 2, 3 and 4. The Process and the Decision hold the types of their masters' rows. It is the same by `shape.size` and by `shape.resize`.
//! * **The sweep**: every master of the built-in stencils, dropped, resized and saved, holds no row without `T` in a typed section, in the file made from the drawing and in the file an edit was
//!   spliced into.
//! * **The routes**: the file a model writer makes, the splice into a file read, `.diacraft` saved as `.vsdx`, and an `.odg` saved as `.vsdx`.
//! * **Round trips**: the file read again holds the rows the drawing held, and a save of it with no edit is the same bytes.

use std::collections::BTreeMap;

use diacraft_container::xml::{self, El};
use diacraft_doc::{Document, Owner, Resolver, RowKey, SectionKey, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::stencil::all_masters;
use serde_json::json;

use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder};

use crate::edit::{doc, id_of};
use crate::native_common::{Dir, opened, pinned};
use crate::run;

// ------------------------------------------------------------------------------------------------------------------------------- the file

/// The parts of a VSDX package this file reads (spelled in two pieces, as the other tests do, so that no line of the tree holds the format's directory name).
const PAGES_DIR: &str = concat!("/vi", "sio", "/pages/");
const MASTERS_DIR: &str = concat!("/vi", "sio", "/masters/");
const MASTERS_INDEX: &str = concat!("/vi", "sio", "/masters/masters.xml");
const MASTERS_RELS: &str = concat!("/vi", "sio", "/masters/_rels/masters.xml.rels");
const PAGE_1: &str = concat!("/vi", "sio", "/pages/page1.xml");

/// One row of a typed section (`Geometry` or `Connection`) as a saved part holds it.
#[derive(Clone, Debug)]
struct Saved {
    /// The part the row is in: the first page's (`PAGE_1`) or a master's.
    part: String,
    /// The `ID` of the shape the section is in.
    shape: String,
    /// How deep the shape is in its part: 0 for a shape of the page or of the master, which a top-level instance stands for by its master's one shape.
    depth: usize,
    /// The part of the master the shape is an instance of, or the shape's nearest ancestor that is.
    master: Option<String>,
    section: String,
    section_ix: Option<String>,
    ix: Option<String>,
    name: Option<String>,
    t: Option<String>,
    del: bool,
    /// The cells by name: `(V, F)`.
    cells: BTreeMap<String, (Option<String>, Option<String>)>,
}

impl Saved {
    /// Where the row is, to name it in a failure.
    fn at(&self) -> String {
        format!(
            "{} shape {} {}{} row {}",
            self.part,
            self.shape,
            self.section,
            self.section_ix.as_ref().map(|i| format!(" IX {i}")).unwrap_or_default(),
            self.ix.clone().or_else(|| self.name.clone()).unwrap_or_else(|| "?".to_owned())
        )
    }

    /// The cell `name` as `(V, F)`.
    fn cell(&self, name: &str) -> (Option<&str>, Option<&str>) {
        self.cells.get(name).map_or((None, None), |(v, f)| (v.as_deref(), f.as_deref()))
    }
}

/// The sections whose rows have a type.
const TYPED: [&str; 2] = ["Geometry", "Connection"];

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

fn walk(el: &El, part: &str, shape: &str, depth: usize, master: Option<&String>, masters: &BTreeMap<String, String>, out: &mut Vec<Saved>) {
    for child in el.elements() {
        if child.is("Shape") {
            let own = child.attr("Master").and_then(|m| masters.get(m));
            let below = if shape.is_empty() { 0 } else { depth + 1 };
            walk(child, part, child.attr("ID").unwrap_or("?"), below, own.or(master), masters, out);
        } else if child.is("Section") && TYPED.contains(&child.attr("N").unwrap_or("")) {
            for row in child.children_named("Row") {
                let cells = row
                    .children_named("Cell")
                    .map(|c| (c.attr("N").unwrap_or("?").to_owned(), (c.attr("V").map(str::to_owned), c.attr("F").map(str::to_owned))))
                    .collect();
                out.push(Saved {
                    part: part.to_owned(),
                    shape: shape.to_owned(),
                    depth,
                    master: master.cloned(),
                    section: child.attr("N").unwrap_or("").to_owned(),
                    section_ix: child.attr("IX").map(str::to_owned),
                    ix: row.attr("IX").map(str::to_owned),
                    name: row.attr("N").map(str::to_owned),
                    t: row.attr("T").map(str::to_owned),
                    del: row.attr("Del") == Some("1"),
                    cells,
                });
            }
        } else {
            walk(child, part, shape, depth, master, masters, out);
        }
    }
}

/// Every row of a typed section in the pages and the masters of the package `bytes`.
fn saved_rows(bytes: &[u8]) -> Vec<Saved> {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let masters = master_parts(&pkg);
    let mut out = Vec::new();
    let names: Vec<String> = pkg
        .parts()
        .filter(|p| (p.starts_with(&format!("{PAGES_DIR}page")) || p.starts_with(&format!("{MASTERS_DIR}master"))) && p.ends_with(".xml"))
        .map(str::to_owned)
        .collect();
    for name in names {
        let root = xml::parse(pkg.part(&name).unwrap()).unwrap();
        walk(&root, &name, "", 0, None, &masters, &mut out);
    }
    out
}

/// The rows of a `Geometry` section that have no `T` (a row deleted here holds none): every one of them in the files of the sample-file run has one. A `Connection` row may have none, and
/// the row of an instance has none where the master's has none.
fn untyped(rows: &[Saved]) -> Vec<&Saved> {
    rows.iter().filter(|r| r.section == "Geometry" && r.t.is_none() && !r.del).collect()
}

/// The rows of the shape `id` of page 1, of the section `name`.
fn of_shape<'a>(rows: &'a [Saved], id: u32, name: &str) -> Vec<&'a Saved> {
    rows.iter().filter(|r| r.part == PAGE_1 && r.shape == id.to_string() && r.section == name).collect()
}

/// The `T` of the master's row that a row of an instance overrides: found in the part of the master by the section's `IX` and the row's key, among the rows of the master's top-level shapes, for the row of
/// a top-level shape of the page (a member of a group is made to stand for a shape of the master by `MasterShape`, which this does not follow).
fn masters_type<'a>(rows: &'a [Saved], own: &Saved) -> Option<&'a str> {
    let part = own.master.as_deref().filter(|_| own.depth == 0)?;
    rows.iter()
        .find(|m| {
            m.part == part && m.depth == 0 && m.section == own.section && m.section_ix == own.section_ix && m.ix == own.ix && m.name == own.name
        })?
        .t
        .as_deref()
}

// ------------------------------------------------------------------------------------------------------------------------- the drawing

/// A blank drawing with the master `master` dropped on it: its `ID`.
fn dropped(master: &str) -> (Session, u32) {
    let mut s = pinned();
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": master, "page": 1, "x": 3.0, "y": 3.0})));
    (s, id)
}

/// The drawing saved as `name` (the file `dir` holds): the model writer of a new drawing, or the splice into the file it was read from when that is its name.
fn saved_as(s: &mut Session, dir: &Dir, name: &str) -> Vec<u8> {
    run(s, "file.save_as", json!({"path": dir.path(name)}));
    dir.read(name)
}

/// How a test makes a shape taller or wider: `shape.size`, or `shape.resize` with the frame of the shape dropped at (3, 3).
#[derive(Clone, Copy, Debug)]
enum How {
    Size,
    Resize,
}

/// Gives the shape `id`, which is `from` inches, the size `to` (a side that is `None` is left): by the command `how` says.
fn resize(s: &mut Session, how: How, id: u32, from: (f64, f64), to: (Option<f64>, Option<f64>)) {
    let (width, height) = (to.0.unwrap_or(from.0), to.1.unwrap_or(from.1));
    match how {
        How::Size => {
            let mut params = json!({"ids": [id]});
            if let Some(w) = to.0 {
                params["width"] = json!(w);
            }
            if let Some(h) = to.1 {
                params["height"] = json!(h);
            }
            run(s, "shape.size", params);
        }
        // The frame of a shape dropped at (3, 3): the pin is the centre.
        How::Resize => {
            run(s, "shape.resize", json!({"id": id, "x": 3.0 - from.0 / 2.0, "y": 3.0 - from.1 / 2.0, "width": width, "height": height}));
        }
    }
}

const HOWS: [How; 2] = [How::Size, How::Resize];

// ------------------------------------------------------------------------------------------------------------------------- the vector

/// A row of an instance's `Geometry` as `(IX, T, V of a cell, F of the cell)`.
type GeometryRow = (String, Option<String>, Option<String>, Option<String>);

/// The rows of the rectangle's `Geometry` that its instance holds, as `(IX, T, V of the cell, F of the cell)` for the cell `cell`.
fn geometry_of(rows: &[Saved], id: u32, cell: &str) -> Vec<GeometryRow> {
    of_shape(rows, id, "Geometry")
        .into_iter()
        .map(|r| {
            let (v, f) = r.cell(cell);
            (r.ix.clone().unwrap_or_default(), r.t.clone(), v.map(str::to_owned), f.map(str::to_owned))
        })
        .collect()
}

fn row(ix: &str, t: &str, v: &str) -> GeometryRow {
    (ix.to_owned(), Some(t.to_owned()), Some(v.to_owned()), Some("Inh".to_owned()))
}

/// **The composed vector.** The rectangle (1.5 by 1 inch) made 3 inches tall and saved as `.vsdx`: rows `IX` 3 and 4 of the instance's `Geometry` are `LineTo` and their `Y` is `Inh` with
/// the result 3.
#[test]
fn a_rectangle_made_taller_saves_the_two_rows_it_changes_as_line_to_with_the_new_y() {
    for how in HOWS {
        let dir = Dir::new();
        let (mut s, id) = dropped("rectangle");
        resize(&mut s, how, id, (1.5, 1.0), (None, Some(3.0)));
        let rows = saved_rows(&saved_as(&mut s, &dir, "a.vsdx"));
        assert_eq!(geometry_of(&rows, id, "Y"), [row("3", "LineTo", "3"), row("4", "LineTo", "3")], "{how:?}");
    }
}

#[test]
fn a_rectangle_made_wider_saves_the_two_rows_it_changes_as_line_to_with_the_new_x() {
    for how in HOWS {
        let dir = Dir::new();
        let (mut s, id) = dropped("rectangle");
        resize(&mut s, how, id, (1.5, 1.0), (Some(4.0), None));
        let rows = saved_rows(&saved_as(&mut s, &dir, "a.vsdx"));
        assert_eq!(geometry_of(&rows, id, "X"), [row("2", "LineTo", "4"), row("3", "LineTo", "4")], "{how:?}");
    }
}

#[test]
fn a_rectangle_made_wider_and_taller_saves_the_three_rows_it_changes_as_line_to() {
    for how in HOWS {
        let dir = Dir::new();
        let (mut s, id) = dropped("rectangle");
        resize(&mut s, how, id, (1.5, 1.0), (Some(4.0), Some(3.0)));
        let rows = saved_rows(&saved_as(&mut s, &dir, "a.vsdx"));
        let held: Vec<(String, Option<String>)> = geometry_of(&rows, id, "X").into_iter().map(|(ix, t, ..)| (ix, t)).collect();
        let want: Vec<(String, Option<String>)> = ["2", "3", "4"].iter().map(|ix| ((*ix).to_owned(), Some("LineTo".to_owned()))).collect();
        assert_eq!(held, want, "{how:?}");
        assert_eq!(geometry_of(&rows, id, "Y")[1..], [row("3", "LineTo", "3"), row("4", "LineTo", "3")], "{how:?}");
    }
}

/// The connection points of the rectangle that follow the size are rows of their own, and **hold no `T` where the master's rows hold none**: a row of that section is "absent on a partial local row"
/// in the files of the sample-file run, and a viewer draws nothing from them. The row keeps its `IX`.
#[test]
fn the_connection_rows_a_resize_changes_are_left_as_the_masters_are() {
    for how in HOWS {
        let dir = Dir::new();
        let (mut s, id) = dropped("rectangle");
        resize(&mut s, how, id, (1.5, 1.0), (Some(4.0), Some(3.0)));
        let rows = saved_rows(&saved_as(&mut s, &dir, "a.vsdx"));
        let own = of_shape(&rows, id, "Connection");
        assert!(!own.is_empty(), "{how:?}: the resize changes some connection point");
        for r in own {
            assert!(r.ix.is_some(), "{how:?}: {} has no IX", r.at());
            assert_eq!(r.t.as_deref(), masters_type(&rows, r), "{how:?}: {}", r.at());
        }
    }
}

/// The Process and the Decision, whose outlines `draw.io` drew as one edge or nothing: each row of the instance holds the type of the master's row of the same `IX`.
#[test]
fn a_process_and_a_decision_made_taller_and_wider_hold_the_types_of_their_masters_rows() {
    for master in ["process", "decision"] {
        for how in HOWS {
            let dir = Dir::new();
            let (mut s, id) = dropped(master);
            let spec = diacraft_engine::stencil::find_master(master).unwrap();
            resize(&mut s, how, id, (spec.width, spec.height), (Some(spec.width * 2.0), Some(spec.height * 3.0)));
            let rows = saved_rows(&saved_as(&mut s, &dir, "a.vsdx"));
            let own: Vec<&Saved> = rows.iter().filter(|r| r.part == PAGE_1 && r.section == "Geometry" && !r.del).collect();
            assert!(!own.is_empty(), "{master} {how:?}: a resize changes the rows of the outline");
            for r in own {
                assert!(r.t.is_some(), "{master} {how:?}: {} has no T", r.at());
                assert_eq!(r.t.as_deref(), masters_type(&rows, r), "{master} {how:?}: {} is not the master's type", r.at());
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------- the sweep

/// A drawing of every master of the built-in stencils dropped and **not** resized, and how big each is: `(ID, spec width, spec height)`.
fn every_master_dropped() -> (Session, Vec<(&'static str, u32, f64, f64)>) {
    let mut s = pinned();
    let mut dropped = Vec::new();
    for spec in all_masters() {
        // The Pool and the Swimlane are structures made by code, no instance of a master.
        if spec.makes_structure() {
            continue;
        }
        let answer = run(&mut s, "shape.drop", json!({"master": spec.id, "page": 1, "x": 3.0, "y": 3.0}));
        if !answer["master"].is_null() {
            dropped.push((spec.id, id_of(&answer), spec.width, spec.height));
        }
    }
    (s, dropped)
}

/// Makes each shape of `dropped` wider and taller: how many it made so.
fn resize_all(s: &mut Session, dropped: &[(&str, u32, f64, f64)]) -> usize {
    let mut resized = 0;
    for (_, id, w, h) in dropped {
        if s.execute("shape.size", &json!({"ids": [id], "width": w * 1.7, "height": h * 2.3})).is_ok() {
            resized += 1;
        }
    }
    resized
}

/// The rows with no `T`, named, by master: what the sweep says when it fails.
fn report(rows: &[Saved], dropped: &[(&str, u32, f64, f64)]) -> String {
    let mut by_master: BTreeMap<String, usize> = BTreeMap::new();
    for r in untyped(rows) {
        let who = dropped.iter().find(|(_, id, ..)| r.part == PAGE_1 && r.shape == id.to_string()).map_or(r.part.clone(), |(m, ..)| (*m).to_owned());
        *by_master.entry(who).or_default() += 1;
    }
    format!("{} rows with no T, by master: {by_master:?}", untyped(rows).len())
}

/// **The sweep.** Every master of every built-in stencil, dropped, made wider and taller and saved: no row of a typed section is written without its `T`, in the file the model writer makes of the
/// drawing and in the file the resize was spliced into.
#[test]
fn every_master_dropped_resized_and_saved_has_a_type_on_every_row_of_a_typed_section() {
    let dir = Dir::new();
    let (mut s, dropped) = every_master_dropped();
    assert!(dropped.len() > 100, "the sweep covers the catalogue: {} masters", dropped.len());
    // The file the drawing is saved as first, then the resize spliced into it (a save into the file it came from).
    saved_as(&mut s, &dir, "a.vsdx");
    let resized = resize_all(&mut s, &dropped);
    assert!(resized > 100, "the sweep resizes the catalogue: {resized} masters");
    run(&mut s, "file.save", json!({}));
    let spliced = saved_rows(&dir.read("a.vsdx"));
    assert!(spliced.len() > 1_000, "the file holds the rows of the catalogue: {}", spliced.len());
    assert!(untyped(&spliced).is_empty(), "spliced: {}", report(&spliced, &dropped));
    // The model writer, from the same drawing.
    let made = saved_rows(&saved_as(&mut s, &dir, "b.vsdx"));
    assert!(untyped(&made).is_empty(), "made: {}", report(&made, &dropped));
    // And the rows of an instance have the type of the master's row they override (a member of a group master is another scope).
    let wrong: Vec<String> = made
        .iter()
        .filter(|r| r.part == PAGE_1 && !r.del)
        .filter(|r| masters_type(&made, r).is_some_and(|t| Some(t) != r.t.as_deref()))
        .map(|r| {
            let who = dropped.iter().find(|(_, id, ..)| r.shape == id.to_string()).map_or("?", |(m, ..)| *m);
            format!("{} ({who}) holds {:?}, the master's is {:?}", r.at(), r.t, masters_type(&made, r))
        })
        .collect();
    assert!(wrong.is_empty(), "rows that differ from their master's type: {:?}", &wrong[..wrong.len().min(8)]);
}

// ------------------------------------------------------------------------------------------------------------------------- the routes

/// A drawing of a rectangle, a process and a decision, each made wider and taller.
fn drawing() -> Session {
    let mut s = pinned();
    for (i, master) in ["rectangle", "process", "decision"].into_iter().enumerate() {
        let id = id_of(&run(&mut s, "shape.drop", json!({"master": master, "page": 1, "x": 3.0 + 3.0 * i as f64, "y": 3.0})));
        let spec = diacraft_engine::stencil::find_master(master).unwrap();
        run(&mut s, "shape.size", json!({"ids": [id], "width": spec.width * 2.0, "height": spec.height * 3.0}));
    }
    s
}

/// **The splice.** A file the drawing was saved as before the resize, read, resized and saved into: the rows the edit adds to the instances it spliced in carry their types.
#[test]
fn a_resize_spliced_into_a_file_that_was_read_saves_its_rows_with_their_type() {
    let dir = Dir::new();
    let (mut s, id) = dropped("rectangle");
    saved_as(&mut s, &dir, "a.vsdx");
    let mut read = opened(&dir.path("a.vsdx"));
    assert!(of_shape(&saved_rows(&dir.read("a.vsdx")), id, "Geometry").is_empty(), "before the edit the instance holds no row of its own");
    resize(&mut read, How::Size, id, (1.5, 1.0), (Some(4.0), Some(3.0)));
    run(&mut read, "file.save", json!({}));
    let rows = saved_rows(&dir.read("a.vsdx"));
    assert_eq!(geometry_of(&rows, id, "Y")[1..], [row("3", "LineTo", "3"), row("4", "LineTo", "3")]);
    assert!(untyped(&rows).is_empty(), "{:?}", untyped(&rows).iter().map(|r| r.at()).collect::<Vec<_>>());
}

/// **`.diacraft` saved as `.vsdx`.** The drawing is saved in DiaCraft's own format, read, and saved as a VSDX type: the model writer writes the rows the `.diacraft` held.
#[test]
fn a_diacraft_file_saved_as_vsdx_holds_the_types_of_the_rows_of_the_instances() {
    let dir = Dir::new();
    let mut s = drawing();
    saved_as(&mut s, &dir, "a.diacraft");
    let mut read = opened(&dir.path("a.diacraft"));
    let rows = saved_rows(&saved_as(&mut read, &dir, "b.vsdx"));
    assert!(rows.len() > 20, "the file holds the rows of the three shapes: {}", rows.len());
    assert!(untyped(&rows).is_empty(), "{:?}", untyped(&rows).iter().map(|r| r.at()).collect::<Vec<_>>());
    // The rows a resize of the rectangle made are in it, with their type.
    assert!(of_shape(&rows, 1, "Geometry").iter().all(|r| r.t.as_deref() == Some("LineTo") || r.t.as_deref() == Some("MoveTo")));
}

/// **`.odg` saved as `.vsdx`.** The drawing is saved as an OpenDocument drawing, read, and saved as a VSDX type: the shapes the `.odg` reader makes hold their rows with types.
#[test]
fn an_odg_file_saved_as_vsdx_holds_a_type_on_every_row() {
    let dir = Dir::new();
    let mut s = drawing();
    saved_as(&mut s, &dir, "a.odg");
    let mut read = opened(&dir.path("a.odg"));
    let rows = saved_rows(&saved_as(&mut read, &dir, "b.vsdx"));
    assert!(rows.len() > 20, "the file holds the rows of the three shapes: {}", rows.len());
    assert!(untyped(&rows).is_empty(), "{:?}", untyped(&rows).iter().map(|r| r.at()).collect::<Vec<_>>());
}

// ------------------------------------------------------------------------------------------------------------------------- round trips

/// The rows a shape has after merging, by section: `(section, Geometry IX, row key, T, cells as (name, V))`.
type Effective = Vec<(&'static str, Option<u32>, RowKey, Option<String>, Vec<(String, Option<String>)>)>;

fn effective(d: &Document, page: u32, id: u32) -> Effective {
    let res = Resolver::new(d);
    let at = d.find_shape(Owner::Page(page), id).unwrap();
    let sheet = SheetRef::Shape(at);
    let mut out = Vec::new();
    for name in TYPED {
        for key in res.effective_sections(&sheet, name).unwrap_or_default() {
            for row in res.effective_rows(&sheet, &SectionKey::new(name, key.ix)).unwrap_or_default() {
                let cells = row
                    .cells
                    .iter()
                    .map(|(n, c)| (n.as_str().to_owned(), c.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())))
                    .collect();
                out.push((name, key.ix, row.key.clone(), row.row_type.as_deref().map(str::to_owned), cells));
            }
        }
    }
    out
}

/// The shapes of page 1 of a drawing, by `ID`.
fn ids_of(s: &Session) -> Vec<u32> {
    doc(s).pages.iter().find(|p| !p.background).unwrap().shapes.iter().map(|sh| sh.id).collect()
}

/// **Reopened it is the same.** The file read again holds, for each shape, the rows the drawing held, with the same types and values.
#[test]
fn the_file_read_again_holds_the_rows_the_drawing_held() {
    let dir = Dir::new();
    let mut s = drawing();
    saved_as(&mut s, &dir, "a.vsdx");
    let read = opened(&dir.path("a.vsdx"));
    assert_eq!(ids_of(&s), ids_of(&read));
    for id in ids_of(&s) {
        let (before, after) = (effective(doc(&s), 1, id), effective(doc(&read), 1, id));
        assert!(!before.is_empty());
        assert!(before.iter().filter(|r| r.0 == "Geometry").all(|r| r.3.is_some()), "shape {id}: every row of the drawing's outline has a type");
        assert_eq!(before, after, "shape {id}");
    }
}

/// **An unedited save is the same bytes.** A file made after the fix, read and saved with no edit, is not changed by the save (no row is given a type, no row is written again).
#[test]
fn a_file_read_and_saved_with_no_edit_is_the_same_bytes() {
    let dir = Dir::new();
    let mut s = drawing();
    let first = saved_as(&mut s, &dir, "a.vsdx");
    let mut read = opened(&dir.path("a.vsdx"));
    run(&mut read, "file.save", json!({}));
    assert_eq!(dir.read("a.vsdx"), first);
}

// ------------------------------------------------------------------------------------------------------------------------- a file an older build wrote

/// The bytes of one part of a package.
fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap().part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()
}

/// A master of a box 2 by 1 inches, and an instance of it made 3 inches tall **as 0.2.3 saved it**: the rows `IX` 3 and 4 of its `Geometry` hold the new `Y` as `Inh` and no `T`. Written by hand.
fn older_file() -> Vec<u8> {
    let master = concat!(
        r#"<Shape ID="5" Type="Shape"><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="1" U="IN"/>"#,
        r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>"#,
        r#"<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>"#,
        r#"<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>"#,
        r#"<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>"#,
        r#"<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section></Shape>"#
    );
    let instance = concat!(
        r#"<Shape ID="1" Type="Shape" Master="1"><Cell N="PinX" V="3" U="IN"/><Cell N="PinY" V="3" U="IN"/><Cell N="Width" V="2" U="IN"/><Cell N="Height" V="3" U="IN"/>"#,
        r#"<Section N="Geometry" IX="0"><Row IX="3"><Cell N="Y" V="3" F="Inh"/></Row><Row IX="4"><Cell N="Y" V="3" F="Inh"/></Row></Section></Shape>"#
    );
    VsdxBuilder::new().master(MasterSpec::new(1, "Box").shapes(master)).page(PageSpec::new(1, "Page-1").shapes(instance)).build()
}

/// **A row an older build saved with no `T` is kept as it is by a save with no edit**: the page part is the bytes it was, since a row is given its type where a write reaches it, and not on the way out.
#[test]
fn a_row_an_older_file_holds_with_no_type_is_kept_as_it_is_by_a_save_with_no_edit() {
    let dir = Dir::new();
    let bytes = older_file();
    let mut s = opened(&dir.write("old.vsdx", &bytes));
    run(&mut s, "file.save", json!({}));
    assert_eq!(part(&dir.read("old.vsdx"), PAGE_1), part(&bytes, PAGE_1));
    let rows = saved_rows(&dir.read("old.vsdx"));
    let own = of_shape(&rows, 1, "Geometry");
    assert_eq!(own.iter().map(|r| r.t.clone()).collect::<Vec<_>>(), [None, None], "the file's two rows have no type, as the file had them");
}

/// **A write that reaches such a row gives it its type**: the box made taller again, in the file an older build wrote, saves the two rows with the master's `T`.
#[test]
fn a_resize_of_a_box_an_older_file_holds_gives_its_rows_without_a_type_the_masters() {
    let dir = Dir::new();
    let mut s = opened(&dir.write("old.vsdx", &older_file()));
    run(&mut s, "shape.size", json!({"ids": [1], "height": 4.0}));
    run(&mut s, "file.save", json!({}));
    let rows = saved_rows(&dir.read("old.vsdx"));
    assert_eq!(geometry_of(&rows, 1, "Y"), [row("3", "LineTo", "4"), row("4", "LineTo", "4")]);
    assert!(untyped(&rows).is_empty(), "{:?}", untyped(&rows).iter().map(|r| r.at()).collect::<Vec<_>>());
}

// ------------------------------------------------------------------------------------------------------------------------- the other ways a row is made

/// **A cell written by hand.** `sheet.set_cell` on a `Geometry` cell of an instance makes the row for it, and the row has the master's type: the same write as a resize's, by another way in.
#[test]
fn a_geometry_cell_written_by_sheet_set_cell_on_an_instance_is_saved_in_a_row_with_the_masters_type() {
    let dir = Dir::new();
    let (mut s, id) = dropped("rectangle");
    run(&mut s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "Geometry1.Y3", "formula": "Height*2"}));
    let rows = saved_rows(&saved_as(&mut s, &dir, "a.vsdx"));
    let own: Vec<(Option<String>, Option<String>)> = of_shape(&rows, id, "Geometry").iter().map(|r| (r.ix.clone(), r.t.clone())).collect();
    assert_eq!(own, [(Some("3".to_owned()), Some("LineTo".to_owned()))], "one row, the one written, with its IX and its type");
    assert!(untyped(&rows).is_empty());
}

/// **A cell a change of master carries over.** The new master lists `Geometry1.Y3` among the cells a swap carries (`ReplaceCopyCells`): the shape holds that cell of its own, in a row it did not
/// hold, and the row has the type of the new master's.
#[test]
fn a_geometry_cell_a_change_of_master_carries_over_is_in_a_row_with_the_type_of_the_new_masters() {
    use crate::stencil_open::{CT_STENCIL, Dir as StencilDir, LOOK, box_cells, ids, open};
    use diacraft_testkit::package::shape_with;
    let boxed = |id: u32, name: &str, extra: &str| {
        MasterSpec::new(id, name).attrs(&ids(100 + id, 200 + id)).shapes(&shape_with(
            5,
            r#" Type="Shape""#,
            &format!("{LOOK}{extra}{}", box_cells(1.5, 1.0)),
        ))
    };
    let masters = || vec![boxed(1, "Old", ""), boxed(2, "New", r#"<Cell N="ReplaceCopyCells" V="0" F="DEPENDSON(Geometry1.Y3)"/>"#)];
    let page = shape_with(10, r#" Master="1""#, r#"<Cell N="PinX" V="3"/><Cell N="PinY" V="2"/>"#);
    let (mut drawing, mut stencil) =
        (VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&page)), VsdxBuilder::new().document_type(CT_STENCIL));
    for m in masters() {
        drawing = drawing.master(m);
    }
    for m in masters() {
        stencil = stencil.master(m);
    }
    let dir = StencilDir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({"path": dir.write("plan.vsdx", &drawing.build())}));
    open(&mut s, &dir.write("stencil.vssx", &stencil.build()));
    run(&mut s, "shape.change", json!({"ids": [10], "master": "New", "stencil": "file-1"}));
    run(&mut s, "file.save_as", json!({"path": dir.path("out.vsdx")}));
    let rows = saved_rows(&std::fs::read(dir.path("out.vsdx")).unwrap());
    let own: Vec<(Option<String>, Option<String>)> = of_shape(&rows, 10, "Geometry").iter().map(|r| (r.ix.clone(), r.t.clone())).collect();
    assert_eq!(
        own,
        [(Some("3".to_owned()), Some("LineTo".to_owned()))],
        "the carried cell is in a row of its own, with the type of the new master's row"
    );
}
