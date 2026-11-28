//! Owner ruling, 2026-11-26: **the least height of a compartment follows the page's drop scale.** The seven masters of compartments (UML Class's `class`,
//! `interface`, `enumeration`, `data-type` and `object`, and Crow's Foot's `entity` and `entity-with-keys`) held that least height as a constant in a formula
//! (`MAX(0.4,TEXTHEIGHT(TheText,Width))`), which a drop beyond the range of eight leaves alone, so a class dropped at 1:20 or 1:100 was 0.85 inch tall on
//! paper and not the master's 1.2. The formula now multiplies the least by the factor the drop wrote (`0.4*IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1)`), so a drop
//! beyond the range keeps the master's size on paper and the text keeps its margin. A drawing whose root has no such cell reads the factor as 1 and keeps the heights it had.
//!
//! * Each of the seven, dropped at 1:1, 1:20 and 1:100, is as tall on paper as the master, and so is each of its compartments.
//! * A text that is taller than the least is as tall on paper at every scale.
//! * A drawing whose root style has no `DropOnPageScale` (an older file) reads 1 where the formula names it.
//! * `.vsdx`, `.diacraft` and `.odg` keep the formula and the heights, and a line typed after the reopen is worked out by it.
//! * The window's ghost (`snap::drop_preview`) is the drop's box.

use diacraft_doc::{CellAddr, CellName, Document, FormulaSlot, Owner, Resolver, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::snap::{DropMaster, drop_preview};
use diacraft_engine::stencil::find_master;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::edit::{doc, id_of};
use crate::stencil_open::Dir;
use crate::{blank_session, check_session, run};

/// The seven masters of compartments.
const SEVEN: [&str; 7] = [
    "uml-class.class",
    "uml-class.interface",
    "uml-class.enumeration",
    "uml-class.data-type",
    "uml-class.object",
    "crows-foot.entity",
    "crows-foot.entity-with-keys",
];

/// The scales, as drawing inches to paper inch: the master's own, and two beyond the range of eight.
const SCALES: [f64; 3] = [1.0, 20.0, 100.0];

/// What the formula of the least height says: the least, times the factor the drop wrote on the group (`Sheet.5`) or 1 where the group has none.
fn least_formula(least: &str) -> String {
    format!("GUARD(MAX({least}*IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1),TEXTHEIGHT(TheText,Width)))")
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0e-9 * a.abs().max(b.abs()).max(1.0)
}

#[track_caller]
fn assert_near(what: &str, got: f64, want: f64) {
    assert!(near(got, want), "{what}: got {got}, want {want}");
}

/// The `ID` of the first page: 1 in a drawing made here, and 0 in one read from an `.odg`.
fn first_page(d: &Document) -> u32 {
    d.pages.get(0).unwrap().id
}

/// The `ID`s of the members of the group `id` of the first page, directly below it, in the master's order.
fn members_of(d: &Document, id: u32) -> Vec<u32> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    d.shape(&at).unwrap().children.iter().map(|m| m.id).collect()
}

/// What a cell of a shape of the first page is, by inheritance.
fn num(d: &Document, id: u32, cell: &str) -> f64 {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap_or_else(|| panic!("no shape {id}"));
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("shape {id} has no {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("shape {id}: {cell} holds no number"))
}

/// What a drop of `master` on a page at `k` drawing inches to the paper inch makes: the group's `ID`.
fn drop_on(s: &mut Session, master: &str, at: (f64, f64)) -> Value {
    run(s, "shape.drop", json!({"master": master, "page": 1, "x": at.0, "y": at.1}))
}

/// A blank drawing whose page is at `k` to 1.
fn page_at(k: f64) -> Session {
    let mut s = blank_session();
    if k != 1.0 {
        run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": k}));
    }
    s
}

/// The height of the group and of each member on paper, in inches, and the width on paper: `(width, height, member heights in the order of the master)`.
fn on_paper(d: &Document, id: u32, k: f64) -> (f64, f64, Vec<f64>) {
    let members = members_of(d, id).into_iter().map(|m| num(d, m, "Height") / k).collect();
    (num(d, id, "Width") / k, num(d, id, "Height") / k, members)
}

/// The formula text of a cell of a shape, as the shape reads it (through its master), `None` for a plain value.
fn formula_of(d: &Document, id: u32, cell: &str) -> Option<String> {
    let at = d.find_shape(Owner::Page(first_page(d)), id)?;
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell))?;
    match &found.formula.cell?.formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

// ---- the seven, at three scales ---------------------------------------------------------------------------------------------------------

/// **The ruling.** Each of the seven, dropped at 1:1, 1:20 and 1:100, is as tall on paper as the master: its answer says so, its cells say so, and so does each compartment. At 1:20 and
/// 1:100 the compartments were as tall as their text only (0.85 inch for a class, 0.59 for an entity), since the least height was 0.02 and 0.004 inch on paper.
#[test]
fn each_of_the_seven_is_as_tall_on_paper_at_1_to_20_and_1_to_100_as_at_1_to_1() {
    for master in SEVEN {
        let spec = find_master(master).unwrap_or_else(|| panic!("no master {master}"));
        let mut whole: Option<(f64, f64, Vec<f64>)> = None;
        for k in SCALES {
            let mut s = page_at(k);
            let r = drop_on(&mut s, master, (5.0, 4.0));
            let id = id_of(&r);
            let (width, height, members) = on_paper(doc(&s), id, k);
            // The answer is the shape's size on paper.
            assert_near(&format!("{master} at 1:{k}: the answer's height"), r["height"].as_f64().unwrap(), height);
            assert_near(&format!("{master} at 1:{k}: the answer's width"), r["width"].as_f64().unwrap(), width);
            // The group is the sum of its compartments.
            assert_near(&format!("{master} at 1:{k}: the sum of the compartments"), members.iter().sum::<f64>(), height);
            // The master's size on paper (its listing says it within a hundredth of an inch: the least heights' sum).
            assert!((height - spec.height).abs() < 0.02, "{master} at 1:{k}: {height} inch tall on paper, the master is {}", spec.height);
            assert!((width - spec.width).abs() < 1.0e-9, "{master} at 1:{k}: {width} inch wide on paper, the master is {}", spec.width);
            match &whole {
                None => whole = Some((width, height, members)),
                Some((w1, h1, m1)) => {
                    assert_near(&format!("{master} at 1:{k}: the width on paper is the master's"), width, *w1);
                    assert_near(&format!("{master} at 1:{k}: the height on paper is the master's"), height, *h1);
                    assert_eq!(members.len(), m1.len(), "{master}");
                    for (i, (a, b)) in members.iter().zip(m1).enumerate() {
                        assert_near(&format!("{master} at 1:{k}: compartment {i} on paper"), *a, *b);
                    }
                }
            }
            assert_eq!(check_session(&s), Ok(()), "{master} at 1:{k}");
        }
    }
}

/// A drop within the range keeps the master's size in drawing units and writes nothing: the least height is the master's, 1.2 inches on the page, and no cell is written.
#[test]
fn a_drop_within_the_range_keeps_the_least_heights_in_drawing_units() {
    for k in [2.0, 4.0, 8.0] {
        let mut s = page_at(k);
        let id = id_of(&drop_on(&mut s, "uml-class.class", (5.0, 4.0)));
        let (_, height, _) = on_paper(doc(&s), id, 1.0);
        assert_near(&format!("1:{k}: the class is as tall in drawing units as the master"), height, find_master("uml-class.class").unwrap().height);
        let group = doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), id).unwrap()).unwrap().clone();
        assert!(!group.sheet.cells.contains_key(&CellName::new("DropOnPageScale")), "1:{k}: nothing written");
    }
}

/// The compartments of a text that is taller than their least are as tall on paper at every scale: the text keeps its size and its margin, so the least heights decide
/// nothing there and the scale changes nothing.
#[test]
fn a_text_taller_than_the_least_is_as_tall_on_paper_at_every_scale() {
    for master in ["uml-class.class", "crows-foot.entity"] {
        let mut heights = Vec::new();
        for k in SCALES {
            let mut s = page_at(k);
            let id = id_of(&drop_on(&mut s, master, (5.0, 4.0)));
            // The member typing goes to is the last in the master's table: the name. The attributes are the one before it in a class, the last but one in an entity.
            let members = members_of(doc(&s), id);
            let list = members[members.len() - 2];
            run(&mut s, "shape.set_text", json!({"id": list, "text": "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight"}));
            let (_, height, parts) = on_paper(doc(&s), id, k);
            assert_near(&format!("{master} at 1:{k}: the group is the sum of its compartments"), parts.iter().sum::<f64>(), height);
            heights.push((k, height, parts));
        }
        let (_, first, parts) = heights[0].clone();
        assert!(first > find_master(master).unwrap().height + 0.5, "{master}: eight lines make the shape taller than the master ({first})");
        for (k, height, other) in &heights[1..] {
            assert!((height - first).abs() < 1.0e-4, "{master} at 1:{k}: {height} inch against {first} at 1:1");
            for (a, b) in other.iter().zip(&parts) {
                assert!((a - b).abs() < 1.0e-4, "{master} at 1:{k}: a compartment is {a} inch against {b} at 1:1");
            }
        }
    }
}

// ---- the formula, and drawings that are older ------------------------------------------------------------------------------------------

/// A drawing made by a file whose root style has no `DropOnPageScale` (every drawing before the ruling on F40): a cell found nowhere reads 0, so a formula that multiplied by it would
/// make every compartment as tall as its text only. The formula reads 1 there, and the least heights are the ones they were.
fn old_root_session(k: f64) -> (Session, Dir) {
    let dir = Dir::new();
    let root = r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/></StyleSheet>"#;
    let sheet = format!(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/><Cell N="PageScale" V="1"/><Cell N="DrawingScale" V="{k}"/>"#);
    let drawing = VsdxBuilder::new().styles(root).page(PageSpec::new(1, "Page-1").sheet(&sheet)).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("old.vsdx", &drawing)}));
    (s, dir)
}

#[test]
fn a_drawing_whose_root_has_no_factor_keeps_the_heights_it_had() {
    // At 1:1 and within the range: the master's least heights, not the heights of the texts.
    for k in [1.0, 4.0] {
        let (mut s, _dir) = old_root_session(k);
        assert!(doc(&s).styles.iter().find(|st| st.id == 0).unwrap().sheet.cell("DropOnPageScale").is_none(), "the root has no factor");
        let id = id_of(&drop_on(&mut s, "uml-class.class", (5.0, 4.0)));
        let (_, height, parts) = on_paper(doc(&s), id, 1.0);
        assert_near(&format!("1:{k}: the class's height (the least heights, 3 times 0.4)"), height, 1.2);
        assert!(parts.iter().all(|h| (h - 0.4).abs() < 1.0e-9), "1:{k}: each compartment is at its least: {parts:?}");
    }
    // Beyond the range the drop writes the factor on the group, so the least heights follow it as in any drawing.
    let (mut s, _dir) = old_root_session(100.0);
    let id = id_of(&drop_on(&mut s, "uml-class.class", (5.0, 4.0)));
    assert_near("1:100: the class's height on paper", on_paper(doc(&s), id, 100.0).1, 1.2);
}

/// What the members' formulas say: the least times the factor of the group, one where the group holds none.
#[test]
fn the_least_height_names_the_groups_factor_and_reads_one_where_it_has_none() {
    let mut s = blank_session();
    let class = id_of(&drop_on(&mut s, "uml-class.interface", (5.0, 4.0)));
    let members = members_of(doc(&s), class);
    let least = ["0.34", "0.34", "0.3", "0.26"];
    assert_eq!(members.len(), least.len());
    // Ranks run from the top, the members stand in the master's own order: each member's formula is the least of its rank.
    let mut seen: Vec<String> = members.iter().map(|m| formula_of(doc(&s), *m, "Height").unwrap()).collect();
    seen.sort();
    let mut want: Vec<String> = least.iter().map(|l| least_formula(l)).collect();
    want.sort();
    assert_eq!(seen, want);
}

// ---- files -------------------------------------------------------------------------------------------------------------------------------

/// The formulas of the `Height` of the members of the master of the group `id` (of the first page): the master's body, where a file keeps a formula. An `.odg` holds the masters with
/// their bodies in its document file (the second tier), and reads an instance's members as plain shapes of their own; a VSDX or `.diacraft` file reads them through the master.
fn master_formulas(d: &Document, id: u32) -> Vec<Option<String>> {
    let at = d.find_shape(Owner::Page(first_page(d)), id).unwrap();
    let group = d.shape(&at).unwrap();
    let master = d.master(group.master.expect("the group is an instance of a master")).unwrap();
    let top = master.shapes.get(0).unwrap();
    top.children.iter().map(|c| c.sheet.cell("Height").and_then(|c| c.formula.text().map(str::to_owned))).collect()
}

/// A class at 1:100 saved in a type and read again: the master holds the same formula (the body of the master in every type) and the group the same factor, the heights are the
/// same, and a second save and read changes nothing. Where the file reads an instance's members through the master (`through_master`: not an `.odg`, which reads them as plain shapes
/// with the sizes the cells had, as it did before the least height followed the scale), the members hold the same formulas and a line typed after the reopen is worked out by them: the
/// least height is still 40 drawing units, so a short line leaves the compartment where it was.
fn trip(file: &str, master: &str, through_master: bool) {
    let dir = Dir::new();
    let mut s = page_at(100.0);
    let id = id_of(&drop_on(&mut s, master, (5.0, 4.0)));
    let before = on_paper(doc(&s), id, 100.0);
    let held = master_formulas(doc(&s), id);
    assert!(
        !held.is_empty() && held.iter().all(|f| f.as_deref().is_some_and(|f| f.contains("Sheet.5!DropOnPageScale"))),
        "{file}: the master names the factor: {held:?}"
    );
    let formulas: Vec<Option<String>> = members_of(doc(&s), id).iter().map(|m| formula_of(doc(&s), *m, "Height")).collect();
    assert!(
        formulas.iter().all(|f| f.as_deref().is_some_and(|f| f.contains("DropOnPageScale"))),
        "{file}: the members name the factor before the save: {formulas:?}"
    );
    let path = dir.path(file);
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let read = doc(&t).pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let after = on_paper(doc(&t), read, 100.0);
    assert_near(&format!("{file} {master}: the height on paper"), after.1, before.1);
    for (a, b) in after.2.iter().zip(&before.2) {
        assert_near(&format!("{file} {master}: a compartment on paper"), *a, *b);
    }
    assert_eq!(master_formulas(doc(&t), read), held, "{file} {master}: the master's formulas are kept");
    let group = doc(&t).shape(&doc(&t).find_shape(Owner::Page(first_page(doc(&t))), read).unwrap()).unwrap().clone();
    assert_near(
        &format!("{file} {master}: the group's factor"),
        group.sheet.cell("DropOnPageScale").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap(),
        100.0,
    );
    // A second save of what was read, and a read of that: the same master, the same heights.
    let again = dir.path(&format!("again-{file}"));
    run(&mut t, "file.save_as", json!({"path": again}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": again}));
    let second = doc(&u).pages.get(0).unwrap().shapes.get(0).unwrap().id;
    assert_eq!(master_formulas(doc(&u), second), held, "{file} {master}: a second trip keeps the master's formulas");
    assert_near(&format!("{file} {master}: a second trip"), on_paper(doc(&u), second, 100.0).1, before.1);
    if through_master {
        let again: Vec<Option<String>> = members_of(doc(&t), read).iter().map(|m| formula_of(doc(&t), *m, "Height")).collect();
        assert_eq!(again, formulas, "{file} {master}: the members' formulas are kept");
        // A line typed in a compartment is worked out by the formula that was kept: it stays at its least.
        let members = members_of(doc(&t), read);
        let list = members[members.len() - 2];
        let page = first_page(doc(&t));
        run(&mut t, "shape.set_text", json!({"id": list, "page": page, "text": "x"}));
        assert_near(&format!("{file} {master}: the height on paper after a line typed"), on_paper(doc(&t), read, 100.0).1, before.1);
    }
    assert_eq!(check_session(&t), Ok(()), "{file} {master}");
}

#[test]
fn the_least_heights_survive_a_save_and_a_reopen_in_vsdx() {
    for master in ["uml-class.class", "crows-foot.entity"] {
        trip("class.vsdx", master, true);
    }
}

#[test]
fn the_least_heights_survive_a_save_and_a_reopen_in_diacraft() {
    for master in ["uml-class.class", "crows-foot.entity"] {
        trip("class.diacraft", master, true);
    }
}

/// An `.odg` keeps the formula in its second tier: the master with its body is in the document file, and the group holds the factor on its own style.
#[test]
fn the_least_heights_survive_a_save_and_a_reopen_in_odg() {
    for master in ["uml-class.class", "crows-foot.entity"] {
        trip("class.odg", master, false);
    }
}

// ---- the window's ghost ------------------------------------------------------------------------------------------------------------------

/// The ghost is the shape the drop makes, and now that shape is the master's size on paper: the preview says 1.2 inches for a class at every scale beyond the range.
#[test]
fn the_ghost_of_each_of_the_seven_is_the_masters_size_on_paper() {
    for master in SEVEN {
        let spec = find_master(master).unwrap();
        for k in SCALES {
            let s = page_at(k);
            let preview = drop_preview(doc(&s), 1, DropMaster::Stock(spec), (5.0, 4.0)).expect("a preview");
            assert!(
                (preview.height() - spec.height).abs() < 0.02,
                "{master} at 1:{k}: the ghost is {} inch tall, the master {}",
                preview.height(),
                spec.height
            );
            assert!((preview.width() - spec.width).abs() < 1.0e-9, "{master} at 1:{k}: the ghost is {} inch wide", preview.width());
        }
    }
}
