//! Owner ruling, 2026-11-27: **every built-in master's fixed inner lengths follow the page's drop scale.** A master dropped on a page whose drawing scale is beyond the
//! range of eight of its own keeps its size on paper, and the owner ruled that its figure must look the same on paper as at 1:1: the lifeline's head, the
//! actor's figure, the tag of a fragment, the markers of BPMN, the label bands of C4, the corner of a rounded box and the tab of a package were constants in the master's formulas, which a drop
//! never scales, so at 1:20 and 1:100 they were a twentieth and a hundredth of their size on paper. Each such length now multiplies the factor the drop wrote (`0.4*IF(DropOnPageScale>0,
//! DropOnPageScale,1)`, and `Sheet.5!DropOnPageScale` in a member of a group, which no drop writes a factor on).
//!
//! * **The sweep.** Every master of every built-in stencil, dropped at 1:1, 1:20, 1:100 and 20:1, has the same lengths on paper: the frame and the text block, every row of every `Geometry` and
//!   `Connection` section and the margin of a container, in the shape and in each member of a group.
//! * **The picture.** The same masters, rendered at paper scale, are drawn the same.
//! * **Within the range nothing changes**: at 1:2, 1:4, 1:8, 2:1 and 8:1 each length is the master's, in drawing units.
//! * **A drawing whose root has no `DropOnPageScale`** (an older file) keeps the figure it had.
//! * **Files.** `.vsdx`, `.diacraft` and `.odg` keep the lengths on paper and the formulas that make them.

use std::collections::BTreeMap;

use diacraft_doc::{CellAddr, Document, Member, Owner, Resolver, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::stencil::{all_masters, find_master};
use diacraft_render::{Rendered, Renderer, View};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::edit::{doc, id_of};
use crate::stencil_open::Dir;
use crate::{blank_session, check_session, run};

/// The scales beyond the range of eight, as drawing inches to a paper inch (a page drawn 20 to 1 is a twentieth of a drawing inch to the paper inch: the master is then the larger), and those
/// within it.
const BEYOND: [f64; 3] = [20.0, 100.0, 0.05];
const WITHIN: [f64; 5] = [2.0, 4.0, 8.0, 0.5, 0.125];

/// Where a master is dropped, in paper inches.
const AT: (f64, f64) = (5.3, 4.1);

pub(crate) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0e-9 * a.abs().max(b.abs()).max(1.0)
}

/// A blank drawing whose page is at `k` to 1.
pub(crate) fn page_at(k: f64) -> Session {
    let mut s = blank_session();
    if k != 1.0 {
        run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": k}));
    }
    s
}

/// The `ID` of the first page: 1 in a drawing made here, and 0 in one read from an `.odg`.
pub(crate) fn first_page(d: &Document) -> u32 {
    d.pages.get(0).unwrap().id
}

/// What a drop of `master` makes on `s`.
pub(crate) fn drop_on(s: &mut Session, master: &str) -> Value {
    run(s, "shape.drop", json!({"master": master, "page": 1, "x": AT.0, "y": AT.1}))
}

// ---- what a shape's lengths are -----------------------------------------------------------------------------------------------------------

/// The cells of the frame and of the text block of a shape, which are lengths.
const FRAME: [&str; 8] = ["Width", "Height", "LocPinX", "LocPinY", "TxtPinX", "TxtPinY", "TxtLocPinX", "TxtLocPinY"];
const TEXT_BLOCK: [&str; 2] = ["TxtWidth", "TxtHeight"];

fn number_of(res: &Resolver<'_>, sheet: &SheetRef, name: &str) -> Option<f64> {
    res.resolve(sheet, &CellAddr::direct(name)).and_then(|found| found.value.cell?.value.as_ref()?.number()).filter(|n| n.is_finite())
}

/// Every length of the shape `at` and of its members, by a name that says where it is: the cells of its frame and text block, the lengths of each row of its `Geometry` and `Connection`
/// sections (the cells whose unit is inches), and the margin a container holds in its `User` rows. The pin of the top-level shape is where it was dropped and is left out, and so are the
/// ends of a one-dimensional shape.
fn lengths(d: &Document, at: &diacraft_doc::ShapeRef, path: &str, top: bool, out: &mut BTreeMap<String, f64>) {
    let res = Resolver::new(d);
    let sheet = SheetRef::Shape(at.clone());
    let mut names: Vec<&str> = FRAME.into_iter().chain(TEXT_BLOCK).collect();
    if !top {
        names.extend(["PinX", "PinY"]);
    }
    for name in names {
        if let Some(n) = number_of(&res, &sheet, name) {
            out.insert(format!("{path}{name}"), n);
        }
    }
    for section in ["Geometry", "Connection", "User"] {
        for key in res.effective_sections(&sheet, section).unwrap_or_default() {
            for (r, row) in res.effective_rows(&sheet, &key).unwrap_or_default().iter().enumerate() {
                for (cell, found) in &row.cells {
                    if !found.formula.cell.is_some_and(|c| c.unit.as_deref() == Some("IN")) {
                        continue;
                    }
                    if let Some(n) = found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).filter(|n| n.is_finite()) {
                        out.insert(format!("{path}{section}{:?}#{r}.{}", key.ix, cell.as_str()), n);
                    }
                }
            }
        }
    }
    for (i, member) in res.effective_members(at).unwrap_or_default().into_iter().enumerate() {
        if let Member::Local(child) = member {
            lengths(d, &child, &format!("{path}m{i}."), false, out);
        }
    }
}

/// The lengths of the shape `id` that a drop on `page` made.
pub(crate) fn lengths_of(d: &Document, id: u32) -> BTreeMap<String, f64> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap_or_else(|| panic!("no shape {id}"));
    let mut out = BTreeMap::new();
    lengths(d, &at, "", true, &mut out);
    out
}

/// Whether the drop of `master` is a shape of the master (BPMN's Pool is a structure made by code, a Swimlane is put into a list of lanes: their answer has no master).
fn is_instance(r: &Value) -> bool {
    !r["master"].is_null()
}

/// The lengths a drop of every master of the built-in stencils makes at the scale `k`, by the master's id: a master whose drop is no instance of it is left out (BPMN's Pool, a structure made by
/// code, is dropped at every scale all the same, and the drawing it makes is checked).
fn all_lengths(k: f64) -> BTreeMap<&'static str, BTreeMap<String, f64>> {
    let mut out = BTreeMap::new();
    for spec in all_masters() {
        let mut s = page_at(k);
        let r = drop_on(&mut s, spec.id);
        if is_instance(&r) {
            out.insert(spec.id, lengths_of(doc(&s), id_of(&r)));
        }
        assert_eq!(check_session(&s), Ok(()), "{} at 1:{k}", spec.id);
    }
    out
}

// ---- the sweep: the lengths on paper ----------------------------------------------------------------------------------------------------

/// **The ruling.** Every master of every built-in stencil, dropped at 1:20, 1:100 and 20:1, has each of its lengths as long on paper as it is at 1:1: the frame, the text block, each row of each
/// section of geometry and of connection points, the margin of a container, in the shape and in every member. The test names each master that fails, with its first lengths that differ.
#[test]
fn every_master_has_the_same_lengths_on_paper_at_1_to_20_and_1_to_100_as_at_1_to_1() {
    let base = all_lengths(1.0);
    assert!(base.len() > 100, "the sweep covers the catalogue: {} masters", base.len());
    let mut failed: Vec<String> = Vec::new();
    for k in BEYOND {
        for (id, want) in &all_lengths(k) {
            let Some(at_one) = base.get(id) else { continue };
            let mut wrong: Vec<String> = Vec::new();
            if at_one.keys().ne(want.keys()) {
                wrong.push(format!("the cells differ: {:?}", at_one.keys().filter(|n| !want.contains_key(*n)).take(3).collect::<Vec<_>>()));
            }
            for (name, n) in want {
                if let Some(one) = at_one.get(name)
                    && !near(n / k, *one)
                {
                    wrong.push(format!("{name}: {} on paper against {one} at 1:1", n / k));
                }
            }
            if !wrong.is_empty() {
                let count = wrong.len();
                wrong.truncate(3);
                failed.push(format!("{id} at 1:{k}: {count} of {} lengths differ, e.g. {}", want.len(), wrong.join("; ")));
            }
        }
    }
    assert!(failed.is_empty(), "{} masters differ on paper:\n{}", failed.len(), failed.join("\n"));
}

/// **Within the range nothing changes**: a drop at 1:2, 1:4, 1:8, 2:1 or 8:1 keeps the master's lengths in drawing units, which are the ones it has at 1:1.
#[test]
fn within_the_range_every_master_keeps_its_lengths_in_drawing_units() {
    let base = all_lengths(1.0);
    let mut failed: Vec<String> = Vec::new();
    for k in WITHIN {
        for (id, got) in &all_lengths(k) {
            let Some(want) = base.get(id) else { continue };
            let wrong: Vec<String> =
                got.iter().filter(|(name, n)| want.get(*name).is_some_and(|one| !near(**n, *one))).map(|(name, n)| format!("{name}: {n}")).collect();
            if got.keys().ne(want.keys()) || !wrong.is_empty() {
                failed.push(format!("{id} at 1:{k}: {:?}", wrong.iter().take(3).collect::<Vec<_>>()));
            }
        }
    }
    assert!(failed.is_empty(), "{} masters changed within the range:\n{}", failed.len(), failed.join("\n"));
}

// ---- the picture ------------------------------------------------------------------------------------------------------------------------

const PX_PER_INCH: f64 = 40.0;

/// A page at paper scale, as the renderer draws it.
fn render(s: &Session) -> Rendered {
    let d = doc(s);
    let page = first_page(d);
    let view = View::fit_page(d, page, PX_PER_INCH).expect("a view of the page");
    let r = Renderer::new().render(d, page, &view);
    assert!(r.skipped.is_empty(), "everything was drawn: {:?}", r.skipped);
    r
}

/// How many pixels differ clearly (a channel by more than 100 of 255), and how many carry ink in either picture. The edge of a curve is anti-aliased from a number that is not the same at a
/// hundredth of the scale, so a pixel on it may differ by a little (up to 14 pixels of 240 on a stick figure's head differ by a fifth of a channel), but never by this much: where a corner is
/// round in one picture and square in the other, or a head is a fifth of an inch high in one and none in the other, a dozen pixels or a thousand differ clearly (measured with the factor of
/// the drop scale left out of every formula: no master that has a fixed length is under 2, and none that has not is over 0).
fn compare(a: &Rendered, b: &Rendered) -> (usize, usize) {
    assert_eq!((a.width, a.height), (b.width, b.height), "the pages are the same size on paper");
    let (mut clear, mut ink) = (0, 0);
    let ((pixels_a, _), (pixels_b, _)) = (a.rgba_premul.as_chunks::<4>(), b.rgba_premul.as_chunks::<4>());
    for (p, q) in pixels_a.iter().zip(pixels_b) {
        let (dp, dq) = (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 700, u32::from(q[0]) + u32::from(q[1]) + u32::from(q[2]) < 700);
        ink += usize::from(dp || dq);
        if p.iter().zip(q).any(|(x, y)| x.abs_diff(*y) > 100) {
            clear += 1;
        }
    }
    (clear, ink)
}

/// Writes the two pictures where `DIACRAFT_FIGURESCALE_DUMP` says, for a person to look at.
fn dump(name: &str, k: f64, a: &Rendered, b: &Rendered) {
    let Ok(dir) = std::env::var("DIACRAFT_FIGURESCALE_DUMP") else { return };
    for (tag, r) in [("one", a), (&format!("{k}"), b)] {
        let file = std::fs::File::create(format!("{dir}/{name}-{tag}.png")).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), r.width, r.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&r.rgba_premul).unwrap();
    }
}

/// **The picture.** Every master that a drop makes an instance of, dropped on a page at 1:20 and at 1:100 and drawn at paper scale, is the picture it is at 1:1: the same pixels, but for the
/// anti-aliasing of an edge. (The sweep above is the exact check, in cells; this one reads what the renderer makes of them.)
#[test]
fn every_master_is_drawn_at_1_to_20_and_1_to_100_as_it_is_at_1_to_1() {
    let mut failed: Vec<String> = Vec::new();
    for spec in all_masters() {
        let mut one = page_at(1.0);
        let r = drop_on(&mut one, spec.id);
        if !is_instance(&r) {
            continue;
        }
        let picture = render(&one);
        for k in BEYOND {
            let mut s = page_at(k);
            drop_on(&mut s, spec.id);
            let other = render(&s);
            let (clear, ink) = compare(&picture, &other);
            dump(spec.id, k, &picture, &other);
            if clear > 1 {
                failed.push(format!("{} at 1:{k}: {clear} pixels differ clearly, of {ink} with ink", spec.id));
            }
        }
    }
    assert!(failed.is_empty(), "{} pictures differ:\n{}", failed.len(), failed.join("\n"));
}

// ---- the formulas ---------------------------------------------------------------------------------------------------------------------------

/// The master of the drawing that a drop of the built-in master `id` copied in (found by its name: a drawing made here holds the masters it was dropped from and no others).
fn master_of<'a>(d: &'a Document, id: &str) -> Option<&'a diacraft_doc::Master> {
    let name = find_master(id)?.name;
    d.masters.iter().find(|m| m.name.as_deref() == Some(name)).map(|m| &**m)
}

/// What the formula of a cell of the master `id` says, found in the master's own shape or in the member `member` of it (`None` for the top-level shape).
fn master_formula(d: &Document, master: &str, member: Option<usize>, cell: &str) -> Option<String> {
    let m = master_of(d, master)?;
    let top = m.shapes.get(0)?;
    let shape = match member {
        None => top,
        Some(i) => top.children.get(i)?,
    };
    shape.sheet.cell(cell).and_then(|c| c.formula.text().map(str::to_owned))
}

/// The factor of a length of a master's figure, as a formula says it: a shape that holds the cell reads its own, and a member of a group the group's, since no drop writes a factor on a
/// member. A cell found nowhere reads 0, so the formula falls back to 1 where there is none.
const OWN: &str = "IF(DropOnPageScale>0,DropOnPageScale,1)";
const GROUP: &str = "IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)";

#[test]
fn a_length_of_a_member_names_the_groups_factor_and_one_of_a_single_shape_its_own() {
    let mut s = blank_session();
    drop_on(&mut s, "uml-sequence.lifeline");
    drop_on(&mut s, "uml-sequence.fragment");
    drop_on(&mut s, "network.comm-link");
    drop_on(&mut s, "uml-class.package");
    let d = doc(&s);
    // The head of a lifeline is 0.4 inch tall, a stem is as tall as the lifeline less the head.
    assert_eq!(master_formula(d, "uml-sequence.lifeline", Some(0), "Height").as_deref(), Some(&*format!("GUARD(MAX(0,Sheet.5!Height-0.4*{GROUP}))")));
    assert_eq!(master_formula(d, "uml-sequence.lifeline", Some(1), "Height").as_deref(), Some(&*format!("GUARD(MIN(0.4*{GROUP},Sheet.5!Height))")));
    // The tag of a fragment: 0.6 by 0.25 inch, its middle 0.3 inch from the left and 0.125 under the top.
    assert_eq!(master_formula(d, "uml-sequence.fragment", Some(1), "Width").as_deref(), Some(&*format!("GUARD(0.6*{GROUP})")));
    assert_eq!(master_formula(d, "uml-sequence.fragment", Some(1), "Height").as_deref(), Some(&*format!("GUARD(0.25*{GROUP})")));
    assert_eq!(master_formula(d, "uml-sequence.fragment", Some(1), "PinX").as_deref(), Some(&*format!("GUARD(0.3*{GROUP})")));
    // The guard's width is `MAX(0.1,MIN(1,Sheet.5!Width-0.7))`, each length scaled.
    assert_eq!(
        master_formula(d, "uml-sequence.fragment", Some(2), "Width").as_deref(),
        Some(&*format!("GUARD(MAX(0.1*{GROUP},MIN(1*{GROUP},Sheet.5!Width-0.7*{GROUP})))"))
    );
    // A shape of one piece holds the factor itself: the zigzag of a Comm-link is 0.1 inch to each side of its line.
    let zig = master_of(d, "network.comm-link").unwrap();
    let rows = zig.shapes.get(0).unwrap().sheet.sections.iter().find(|sec| sec.name.as_str() == "Geometry").unwrap().rows.clone();
    let ys: Vec<Option<String>> =
        rows.iter().map(|r| r.cells.iter().find(|(n, _)| n.as_str() == "Y").and_then(|(_, c)| c.formula.text().map(str::to_owned))).collect();
    assert!(ys.iter().flatten().any(|f| f == &format!("0.1*{OWN}")), "the zigzag's crest is 0.1 inch times the shape's factor: {ys:?}");
    assert!(ys.iter().flatten().any(|f| f == &format!("-0.1*{OWN}")), "and its trough: {ys:?}");
}

// ---- drawings that are older ----------------------------------------------------------------------------------------------------------

/// A drawing made by a file whose root style has no `DropOnPageScale` (every drawing before the ruling on F40): a cell found nowhere reads 0, so a formula that multiplied by it would take
/// every fixed length away. The formula reads 1 there, and the figure is the one it was.
fn old_root_session(k: f64) -> (Session, Dir) {
    let dir = Dir::new();
    let root = r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/></StyleSheet>"#;
    let sheet = format!(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="{k}"/>"#);
    let drawing = VsdxBuilder::new().styles(root).page(PageSpec::new(1, "Page-1").sheet(&sheet)).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("old.vsdx", &drawing)}));
    (s, dir)
}

/// The masters of a few of each kind of figure: a group with members, a lifeline with its points, a rounded box, a tab, a one-dimensional zigzag and a container.
const KINDS: [&str; 8] = [
    "uml-sequence.lifeline",
    "uml-sequence.actor-lifeline",
    "uml-sequence.fragment",
    "bpmn.task",
    "c4.key",
    "uml-class.package",
    "network.comm-link",
    "c4.software-system",
];

/// The lengths of a drop of `master` on the first page of `s`.
fn dropped_lengths(s: &mut Session, master: &str) -> BTreeMap<String, f64> {
    let id = id_of(&drop_on(s, master));
    lengths_of(doc(s), id)
}

/// The lengths of a drop of `master` in a drawing of the built-in kind, at 1:1.
fn at_one_to_one(master: &str) -> BTreeMap<String, f64> {
    let mut s = blank_session();
    let r = drop_on(&mut s, master);
    lengths_of(doc(&s), id_of(&r))
}

#[test]
fn a_drawing_whose_root_has_no_factor_keeps_the_figures_it_had() {
    let (mut old, _dir) = old_root_session(1.0);
    assert!(doc(&old).styles.iter().find(|st| st.id == 0).unwrap().sheet.cell("DropOnPageScale").is_none(), "the root has no factor");
    for master in KINDS {
        let want = at_one_to_one(master);
        let got = dropped_lengths(&mut old, master);
        assert_eq!(got.len(), want.len(), "{master}");
        for (name, n) in &got {
            assert!(near(*n, want[name]), "{master} {name}: {n} in a drawing with no factor, {} in one that has it", want[name]);
        }
    }
    // Beyond the range the drop writes the factor on the shape, so the figure follows it as in any drawing.
    let (mut old, _dir) = old_root_session(100.0);
    let mut new = page_at(100.0);
    for master in KINDS {
        let a = dropped_lengths(&mut old, master);
        let b = dropped_lengths(&mut new, master);
        assert_eq!(a.len(), b.len(), "{master}");
        for (name, n) in &a {
            assert!(near(*n, b[name]), "{master} {name}: {n} in a drawing with no factor at 1:100, {} in one that has it", b[name]);
        }
    }
}

// ---- files ----------------------------------------------------------------------------------------------------------------------------------

/// How many cells of the shape and of its members hold a formula that names the factor of the drop scale (`DropOnPageScale`): the formulas a master's fixed lengths are written in.
fn factor_formulas(shape: &diacraft_doc::Shape) -> usize {
    let named = |c: &diacraft_doc::Cell| usize::from(c.formula.text().is_some_and(|f| f.contains("DropOnPageScale")));
    let own: usize = shape.sheet.cells.values().map(named).sum();
    let rows: usize = shape
        .sheet
        .sections
        .iter()
        .map(|sec| sec.rows.iter().map(|r| r.cells.values().map(named).sum::<usize>()).sum::<usize>() + sec.cells.values().map(named).sum::<usize>())
        .sum();
    own + rows + shape.children.iter().map(|c| factor_formulas(c)).sum::<usize>()
}

/// A master of each kind dropped at 1:100, saved in `file` and read again: each length on paper is the same, the master the file holds still writes its fixed lengths by the factor (so that a
/// shape dropped from it afterwards, or resized, is worked out by it), and a second save and read changes nothing. A one-dimensional shape's text block is
/// compared too, in every file (an `.odg` gave back the master's numbers for it).
fn trip(file: &str, master: &str) {
    let dir = Dir::new();
    let mut s = page_at(100.0);
    let id = id_of(&drop_on(&mut s, master));
    let before = lengths_of(doc(&s), id);
    let held = factor_formulas(master_of(doc(&s), master).unwrap().shapes.get(0).unwrap());
    assert!(held > 0, "{file} {master}: the master writes a fixed length by the factor");
    let path = dir.path(file);
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let read = doc(&t).pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let after = lengths_of(doc(&t), read);
    for (name, n) in &before {
        if let Some(m) = after.get(name) {
            assert!(near(*m, *n), "{file} {master} {name}: {m} after the reopen, {n} before");
        }
    }
    assert!(after.len() >= before.len() / 2, "{file} {master}: {} lengths read of {}", after.len(), before.len());
    assert_eq!(
        factor_formulas(master_of(doc(&t), master).unwrap().shapes.get(0).unwrap()),
        held,
        "{file} {master}: the master's formulas that name the factor are kept"
    );
    // A second save of what was read, and a read of that: the same lengths, the same master.
    let again = dir.path(&format!("again-{file}"));
    run(&mut t, "file.save_as", json!({"path": again}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": again}));
    let second = doc(&u).pages.get(0).unwrap().shapes.get(0).unwrap().id;
    for (name, n) in &lengths_of(doc(&u), second) {
        if let Some(m) = after.get(name) {
            assert!(near(*m, *n), "{file} {master} {name}: {n} after a second trip, {m} after the first");
        }
    }
    assert_eq!(
        factor_formulas(master_of(doc(&u), master).unwrap().shapes.get(0).unwrap()),
        held,
        "{file} {master}: a second trip keeps the master's formulas"
    );
    assert_eq!(check_session(&t), Ok(()), "{file} {master}");
}

#[test]
fn the_lengths_survive_a_save_and_a_reopen_in_vsdx() {
    for master in KINDS {
        trip("figure.vsdx", master);
    }
}

#[test]
fn the_lengths_survive_a_save_and_a_reopen_in_diacraft() {
    for master in KINDS {
        trip("figure.diacraft", master);
    }
}

#[test]
fn the_lengths_survive_a_save_and_a_reopen_in_odg() {
    for master in KINDS {
        trip("figure.odg", master);
    }
}

/// The first `X` of the first `Geometry` section of the shape `id`: the corner of a rounded box, which starts its outline a corner's length in from the left.
fn first_x(d: &Document, id: u32) -> f64 {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    let rows = Resolver::new(d).effective_rows(&SheetRef::Shape(at), &diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap();
    rows.first().and_then(|row| row.cells.iter().find(|(n, _)| n.as_str() == "X")).and_then(|(_, c)| c.value.cell?.value.as_ref()?.number()).unwrap()
}

/// **A shape resized after a reopen is worked out by the formulas it holds.** A shape of one piece reads its own factor, which every format keeps on the shape, so the corner of a C4 box dropped at
/// 1:100 (0.08 inch, 8 drawing units) is still that after the file is read and the shape is made wider. (A member of a group in an `.odg` is a shape of its own that names its group by the
/// group's `ID`: `scale_follow.rs` holds that.)
#[test]
fn a_shape_resized_after_a_reopen_keeps_its_corner_by_the_factor_in_every_format() {
    for file in ["corner.vsdx", "corner.diacraft", "corner.odg"] {
        let dir = Dir::new();
        let mut s = page_at(100.0);
        let id = id_of(&drop_on(&mut s, "c4.software-system"));
        assert!(near(first_x(doc(&s), id), 8.0), "{file}: the corner is 0.08 inch, 8 drawing units at 1:100");
        let path = dir.path(file);
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut t = Session::new();
        run(&mut t, "file.open", json!({"path": path}));
        let page = first_page(doc(&t));
        let read = doc(&t).page(page).unwrap().shapes.get(0).unwrap().id;
        assert!(near(first_x(doc(&t), read), 8.0), "{file}: the corner after the reopen");
        let f = diacraft_engine::draw::shape_frame(doc(&t), page, read).unwrap();
        run(
            &mut t,
            "shape.resize",
            json!({"id": read, "page": page, "x": f.x / 100.0, "y": f.y / 100.0, "width": f.width * 1.5 / 100.0, "height": f.height / 100.0}),
        );
        let wider = diacraft_engine::draw::shape_frame(doc(&t), page, read).unwrap();
        assert!(near(wider.width, f.width * 1.5), "{file}: the shape is wider ({} drawing units)", wider.width);
        assert!(near(first_x(doc(&t), read), 8.0), "{file}: the corner is still 0.08 inch on paper after the resize");
    }
}
