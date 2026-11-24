//! Master matching and the copy of a master between two documents. The documents are built here, with no session: the same calls are
//! what a paste from one drawing into another makes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, ColorEntry, Document, FaceName, ForeignData, Links, Master, RawAttrs, Row, Section, Shape, ShapeList, Sheet, Style, Value,
};

use super::{Identity, MAX_BRING_NODES, Refusal, bring_master, bring_with, find_same_master};
use crate::stencil::{ensure_master, find_master};

fn cell(value: &str) -> Cell {
    Cell::constant(Value::from_text(value))
}

fn master(id: u32, name: &str, unique: Option<&str>, base: Option<&str>) -> Master {
    let mut raw = RawAttrs::new();
    if let Some(base) = base {
        raw.push("BaseID", base);
    }
    let mut shape = Shape::new(5);
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("Width"), cell("2"));
    shape.sheet = Arc::new(sheet);
    let mut m = Master { id, name: Some(Arc::from(name)), name_u: Some(Arc::from(name)), unique_id: unique.map(Arc::from), raw, ..Master::default() };
    m.shapes.push(Arc::new(shape));
    m
}

fn doc_with(masters: Vec<Master>) -> Document {
    let mut doc = Document::new_blank();
    for m in masters {
        doc.add_master(m).unwrap();
    }
    doc
}

const A: &str = "{AAAAAAAA-0000-4000-8000-000000000001}";
const B: &str = "{BBBBBBBB-0000-4000-8000-000000000002}";
const C: &str = "{CCCCCCCC-0000-4000-8000-000000000003}";
const D: &str = "{DDDDDDDD-0000-4000-8000-000000000004}";
const BOTH: Identity = Identity { unique_is_unique: true, base_is_unique: true };

#[test]
fn a_unique_id_matches_when_the_base_ids_agree_or_one_side_has_none() {
    let source = master(1, "Box", Some(A), Some(B));
    // Same UniqueID and BaseID: the very master.
    assert_eq!(find_same_master(&doc_with(vec![master(7, "Box", Some(A), Some(B))]), &source, BOTH), Some(7));
    // GUIDs are compared in any case, with their blanks trimmed.
    let lower = A.to_ascii_lowercase();
    assert_eq!(find_same_master(&doc_with(vec![master(7, "Box", Some(&format!(" {lower} ")), Some(B))]), &source, BOTH), Some(7));
    // Same UniqueID, another BaseID: another master that happens to hold the number.
    assert_eq!(find_same_master(&doc_with(vec![master(7, "Box", Some(A), Some(C))]), &source, BOTH), None);
    // Same UniqueID, and the document's master has no BaseID: nothing says they differ.
    assert_eq!(find_same_master(&doc_with(vec![master(7, "Box", Some(A), None)]), &source, BOTH), Some(7));
    // Nothing alike.
    assert_eq!(find_same_master(&doc_with(vec![master(7, "Box", Some(C), Some(C))]), &source, BOTH), None);
}

#[test]
fn a_base_id_matches_another_unique_id_and_the_same_name_is_preferred() {
    let source = master(1, "Box", Some(A), Some(B));
    let doc = doc_with(vec![master(3, "Other", Some(C), Some(B)), master(4, "Box", Some(D), Some(B))]);
    assert_eq!(find_same_master(&doc, &source, BOTH), Some(4), "of two with the BaseID the one called the same");
    let doc = doc_with(vec![master(3, "Other", Some(C), Some(B)), master(4, "Else", Some(D), Some(B))]);
    assert_eq!(find_same_master(&doc, &source, BOTH), Some(3), "else the first");
}

#[test]
fn an_id_that_other_masters_of_the_source_hold_matches_nothing_by_itself() {
    let source = master(1, "Box", Some(A), Some(B));
    let doc = doc_with(vec![master(7, "Box", Some(A), Some(B))]);
    // With the UniqueID shared, the BaseID alone decides; with both shared nothing does.
    assert_eq!(find_same_master(&doc, &source, Identity { unique_is_unique: false, base_is_unique: true }), Some(7));
    assert_eq!(find_same_master(&doc, &source, Identity { unique_is_unique: false, base_is_unique: false }), None);
    assert_eq!(find_same_master(&doc, &source, Identity { unique_is_unique: true, base_is_unique: false }), Some(7));
}

#[test]
fn identity_counts_the_ids_of_the_masters_of_a_document() {
    let from = doc_with(vec![
        master(1, "One", Some(A), Some(B)),
        master(2, "Two", Some(A), Some(C)),
        master(3, "Three", Some(C), Some(C)),
        master(4, "Four", None, None),
    ]);
    assert_eq!(Identity::within(&from, 0), Identity { unique_is_unique: false, base_is_unique: true });
    assert_eq!(Identity::within(&from, 1), Identity { unique_is_unique: false, base_is_unique: false });
    assert_eq!(Identity::within(&from, 2), Identity { unique_is_unique: true, base_is_unique: false });
    assert_eq!(Identity::within(&from, 3), Identity { unique_is_unique: false, base_is_unique: false }, "no id says nothing");
    assert_eq!(Identity::within(&from, 9), Identity { unique_is_unique: false, base_is_unique: false });
}

/// A source document with a style 3 based on 4, a colour 25, a face, and a master that uses them all.
fn source_with_parts() -> Document {
    let mut from = Document::new_blank();
    let mut base = Sheet::default();
    base.cells.insert(CellName::new("LineWeight"), cell("0.02"));
    from.add_style(Style { id: 4, name: Some(Arc::from("Base")), name_u: Some(Arc::from("Base")), sheet: Arc::new(base), ..Style::default() })
        .unwrap();
    let mut normal = Sheet { links: Links { line: Some(4), fill: Some(4), text: Some(4) }, ..Sheet::default() };
    normal.cells.insert(CellName::new("FillForegnd"), cell("25"));
    from.add_style(Style { id: 3, name: Some(Arc::from("Normal")), name_u: Some(Arc::from("Normal")), sheet: Arc::new(normal), ..Style::default() })
        .unwrap();
    Arc::make_mut(&mut from.colors).push(ColorEntry { ix: 25, rgb: Arc::from("#3366CC") });
    Arc::make_mut(&mut from.fonts).push(FaceName {
        name_u: Arc::from("Segoe Script"),
        raw: RawAttrs::from_pairs([("Flags", "1")]),
        ext: Default::default(),
    });
    let mut m = master(1, "Box", Some(A), Some(B));
    let mut shape = Shape::new(5);
    let mut sheet = Sheet { links: Links { line: Some(3), fill: Some(3), text: Some(3) }, ..Sheet::default() };
    sheet.cells.insert(CellName::new("LineColor"), cell("25"));
    let mut character = Section::new("Character", None);
    let mut row = Row { ix: Some(0), ..Row::default() };
    row.cells.insert(CellName::new("Font"), cell("Segoe Script"));
    row.cells.insert(CellName::new("Color"), cell("25"));
    character.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(character));
    shape.sheet = Arc::new(sheet);
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    from
}

#[test]
fn a_master_comes_from_one_document_into_another_with_what_it_depends_on() {
    let from = source_with_parts();
    let before_from = from.clone();
    // The target has a style 3 of its own, a colour 25 of its own, and no face.
    let mut doc = Document::new_blank();
    let mut mine = Sheet::default();
    mine.cells.insert(CellName::new("LineWeight"), cell("0.5"));
    doc.add_style(Style { id: 3, name: Some(Arc::from("Mine")), name_u: Some(Arc::from("Mine")), sheet: Arc::new(mine), ..Style::default() })
        .unwrap();
    Arc::make_mut(&mut doc.colors).push(ColorEntry { ix: 25, rgb: Arc::from("#112233") });
    let brought = bring_master(&mut doc, &from, 0, Identity::within(&from, 0)).unwrap();
    assert!(brought.added);
    assert_eq!((brought.styles_added, brought.colours_added, brought.faces_added), (2, 1, 1));
    assert_eq!(from, before_from, "the source is not touched");
    // The target's own style and colour are as they were.
    assert_eq!(doc.style(3).unwrap().name_u.as_deref(), Some("Mine"));
    assert_eq!(doc.colors.iter().map(|c| (c.ix, c.rgb.to_string())).next(), Some((25, "#112233".to_owned())));
    let copy = doc.master(brought.id).unwrap();
    let shape = copy.shapes.get(0).unwrap();
    let normal = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Normal")).unwrap();
    let base = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Base")).unwrap();
    assert_eq!(shape.sheet.links, Links { line: Some(normal.id), fill: Some(normal.id), text: Some(normal.id) });
    assert_eq!(normal.sheet.links, Links { line: Some(base.id), fill: Some(base.id), text: Some(base.id) });
    // The colour 25 of the source is #3366CC, which the target holds at the next free number, 24.
    let blue = doc.colors.iter().find(|c| &*c.rgb == "#3366CC").unwrap();
    assert_eq!(blue.ix, 24);
    assert_eq!(shape.sheet.cells.get("LineColor").unwrap().value.as_ref().unwrap().text(), "24");
    let row = shape.sheet.sections.first().unwrap().rows.first().unwrap();
    assert_eq!(row.cells.get("Color").unwrap().value.as_ref().unwrap().text(), "24");
    assert_eq!(row.cells.get("Font").unwrap().value.as_ref().unwrap().text(), "Segoe Script", "a face is named, not numbered");
    assert_eq!(normal.sheet.cells.get("FillForegnd").unwrap().value.as_ref().unwrap().text(), "24", "a style's colours follow too");
    assert_eq!(doc.fonts.iter().map(|f| f.name_u.to_string()).collect::<Vec<_>>(), ["Segoe Script"]);
    assert_eq!(doc.fonts.first().unwrap().raw.get("Flags"), Some("1"));
    assert_eq!((copy.unique_id.as_deref(), copy.raw.get("BaseID")), (Some(A), Some(B)));
    assert_eq!(copy.rel_id, None);
    // Again: the same master is found, and nothing is added.
    let styles = doc.styles.len();
    let again = bring_master(&mut doc, &from, 0, Identity::within(&from, 0)).unwrap();
    assert_eq!((again.id, again.added), (brought.id, false));
    assert_eq!(doc.styles.len(), styles);
}

#[test]
fn a_shape_nothing_changes_for_is_shared_and_not_copied() {
    let from = source_with_parts();
    // The target has the very same styles under the same IDs, and the colour: nothing is rewritten, and the master's shapes are the source's.
    let mut doc = Document::new_blank();
    for id in [4, 3] {
        doc.add_style(Style::clone(from.style(id).unwrap())).unwrap();
    }
    doc.colors = Arc::clone(&from.colors);
    let brought = bring_master(&mut doc, &from, 0, Identity::within(&from, 0)).unwrap();
    assert!(brought.added);
    assert_eq!((brought.styles_added, brought.colours_added), (0, 0));
    let (a, b) = (from.masters.get(0).unwrap(), doc.master(brought.id).unwrap());
    assert!(Arc::ptr_eq(a.shapes.get(0).unwrap(), b.shapes.get(0).unwrap()), "no copy of the shape");
    assert!(Arc::ptr_eq(&a.sheet, &b.sheet));
}

#[test]
fn a_copy_that_is_refused_leaves_the_target_as_it_was() {
    let mut from = source_with_parts();
    let mut doc = Document::new_blank();
    let before = doc.clone();
    // A picture.
    let mut picture = master(2, "Logo", Some(C), Some(C));
    let mut shape = Shape::new(5);
    shape.foreign = Some(Arc::new(ForeignData { rel_id: Some(Arc::from("rId7")), ..ForeignData::default() }));
    picture.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(picture).unwrap();
    assert_eq!(bring_master(&mut doc, &from, 1, Identity::within(&from, 1)), Err(Refusal::Pictures(1)));
    assert_eq!(doc, before);
    // No such master.
    assert_eq!(bring_master(&mut doc, &from, 9, BOTH), Err(Refusal::NoSuchMaster));
    // A meter too small for what the master holds.
    assert_eq!(bring_with(&mut doc, &from, 0, BOTH, 3), Err(Refusal::TooBig));
    assert_eq!(doc, before);
    // The words say what is wrong.
    assert!(Refusal::Pictures(2).to_string().contains("2 pictures"));
    assert!(Refusal::TooBig.to_string().contains(&MAX_BRING_NODES.to_string()));
}

#[test]
fn the_meter_is_charged_for_rows_and_cells_and_not_for_calls() {
    // One shape with a section of 10,000 rows of three cells: 40,000 nodes and more, whatever the number of calls.
    let mut from = Document::new_blank();
    let mut m = master(1, "Wide", Some(A), Some(B));
    let mut shape = Shape::new(5);
    let mut section = Section::new("Geometry", Some(0));
    for ix in 0..10_000_u32 {
        let mut row = Row { ix: Some(ix), row_type: Some(Arc::from("LineTo")), ..Row::default() };
        for name in ["X", "Y", "A"] {
            row.cells.insert(CellName::new(name), cell("1"));
        }
        section.rows.push(Arc::new(row));
    }
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(section));
    shape.sheet = Arc::new(sheet);
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    let mut doc = Document::new_blank();
    assert_eq!(bring_with(&mut doc, &from, 0, BOTH, 39_999), Err(Refusal::TooBig));
    assert_eq!(doc, Document::new_blank());
    assert!(bring_with(&mut doc, &from, 0, BOTH, 45_000).unwrap().added);
}

#[test]
fn master_ids_are_one_above_the_highest_and_a_unique_id_is_not_repeated() {
    let from = doc_with(vec![master(1, "Box", Some(A), Some(B))]);
    let mut doc = doc_with(vec![master(0, "Zero", Some(C), Some(C)), master(5, "Five", Some(A), Some(C))]);
    let brought = bring_master(&mut doc, &from, 0, Identity::within(&from, 0)).unwrap();
    assert_eq!(brought.id, 6);
    let copy = doc.master(6).unwrap();
    assert_ne!(copy.unique_id.as_deref(), Some(A), "the document's master 5 holds that UniqueID, and is another master");
    let ids: std::collections::HashSet<_> = doc.masters.iter().filter_map(|m| m.unique_id.as_deref().map(str::to_owned)).collect();
    assert_eq!(ids.len(), 3);
    let mut only_zero = doc_with(vec![master(0, "Zero", None, None)]);
    assert_eq!(bring_master(&mut only_zero, &from, 0, BOTH).unwrap().id, 1, "ids start at 1 even when the document holds a master 0");
}

#[test]
fn the_built_in_masters_are_matched_by_the_same_rule() {
    let spec = find_master("rectangle").unwrap();
    let mut doc = Document::new_blank();
    let first = ensure_master(&mut doc, spec).unwrap();
    assert_eq!(ensure_master(&mut doc, spec).unwrap(), first);
    let unique = doc.master(first).unwrap().unique_id.clone().unwrap();
    // A master of the document that holds the built-in UniqueID and another BaseID is another master: the copy takes a UniqueID of its own.
    let mut other = Document::new_blank();
    other.add_master(master(1, "Stranger", Some(&unique), Some(C))).unwrap();
    let id = ensure_master(&mut other, spec).unwrap();
    assert_ne!(id, 1);
    assert_ne!(other.master(id).unwrap().unique_id, other.master(1).unwrap().unique_id);
    assert_eq!(other.master(id).unwrap().raw.get("BaseID"), Some(&*unique), "and keeps its BaseID");
}

#[test]
fn a_formula_that_calls_use_is_noted_and_one_that_ends_in_use_is_not() {
    let mut from = Document::new_blank();
    let mut m = master(1, "Uses", Some(A), Some(B));
    let mut shape = Shape::new(5);
    let mut sheet = Sheet::default();
    sheet.cells.insert(CellName::new("FillPattern"), Cell::with_formula("USE(\"Other\")", Some(Value::from_text("1"))));
    sheet.cells.insert(CellName::new("LinePattern"), Cell::with_formula("REUSE(1)", Some(Value::from_text("1"))));
    shape.sheet = Arc::new(sheet);
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    let mut doc = Document::new_blank();
    let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap();
    assert_eq!(brought.notes.len(), 1);
    assert!(brought.notes.first().unwrap().contains("USE()"));
}

/// A small deterministic generator (xorshift), so that a failing case can be rebuilt from its number.
struct Rng(u64);

impl Rng {
    fn next(&mut self, below: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        u32::try_from(self.0 % u64::from(below.max(1))).unwrap()
    }
}

/// Styles whose links point anywhere among 0 to 15 (themselves, each other in cycles, nowhere), shapes whose links and colours point anywhere, and
/// tables that overlap or clash, from and into: whatever the case, the copy ends, leaves what the target had, and points only at what exists.
#[test]
fn random_style_graphs_and_colour_tables_copy_without_loops_or_dangling_links() {
    for case in 1..=400_u64 {
        let mut rng = Rng(case.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let link = |rng: &mut Rng| match rng.next(18) {
            16 | 17 => None,
            n => Some(n),
        };
        let build = |rng: &mut Rng, with_master: bool| -> Document {
            let mut doc = Document::new_blank();
            for id in 1..=rng.next(12) {
                if rng.next(5) == 0 {
                    continue;
                }
                let mut sheet = Sheet { links: Links { line: link(rng), fill: link(rng), text: link(rng) }, ..Sheet::default() };
                sheet.cells.insert(CellName::new("LineWeight"), cell(&rng.next(4).to_string()));
                sheet.cells.insert(CellName::new("FillForegnd"), cell(&(20 + rng.next(14)).to_string()));
                let name = format!("S{}", rng.next(4));
                let style = Style {
                    id,
                    name: Some(Arc::from(name.as_str())),
                    name_u: Some(Arc::from(name.as_str())),
                    sheet: Arc::new(sheet),
                    ..Style::default()
                };
                doc.add_style(style).unwrap();
            }
            for _ in 0..rng.next(8) {
                let ix = 24 + rng.next(10);
                if doc.colors.iter().all(|c| c.ix != ix) {
                    Arc::make_mut(&mut doc.colors).push(ColorEntry { ix, rgb: Arc::from(format!("#{:06X}", rng.next(6) * 0x111111).as_str()) });
                }
            }
            if with_master {
                let mut m = master(1, "M", Some(A), Some(B));
                let mut list = Vec::new();
                for k in 0..=rng.next(4) {
                    let mut shape = Shape::new(5 + k);
                    let mut sheet = Sheet { links: Links { line: link(rng), fill: link(rng), text: link(rng) }, ..Sheet::default() };
                    sheet.cells.insert(CellName::new("LineColor"), cell(&(20 + rng.next(14)).to_string()));
                    shape.sheet = Arc::new(sheet);
                    list.push(Arc::new(shape));
                }
                m.shapes = ShapeList::from_vec(list);
                doc.add_master(m).unwrap();
            }
            doc
        };
        let from = build(&mut rng, true);
        let mut doc = build(&mut rng, false);
        let (before_styles, before_colours) = (doc.styles.clone(), doc.colors.clone());
        let Ok(brought) = bring_master(&mut doc, &from, 0, BOTH) else { continue };
        // What the target had is as it was.
        for (old, now) in before_styles.iter().zip(doc.styles.iter()) {
            assert_eq!(old, now, "case {case}: a style of the target changed");
        }
        for (old, now) in before_colours.iter().zip(doc.colors.iter()) {
            assert_eq!(old, now, "case {case}: a colour of the target changed");
        }
        // Every link of the copy and of the styles that came is to a style that is there, or none.
        let exists = |l: Option<u32>| l.is_none_or(|id| doc.style(id).is_some());
        let copy = doc.master(brought.id).unwrap();
        let originals = from.masters.get(0).unwrap();
        for (shape, original) in copy.shapes.iter().zip(originals.shapes.iter()) {
            let l = shape.sheet.links;
            assert!(exists(l.line) && exists(l.fill) && exists(l.text), "case {case}: {l:?}");
            let number = |s: &Shape| s.sheet.cells.get("LineColor").unwrap().value.as_ref().unwrap().number().unwrap() as i64;
            // A colour the source's table holds is the same colour in the target; one it does not hold names nothing there either.
            if let Some(rgb) = from.color(number(original)) {
                assert_eq!(doc.color(number(shape)), Some(rgb), "case {case}: colour {} became {}", number(original), number(shape));
            }
        }
        for style in doc.styles.iter().skip(before_styles.len()) {
            let l = style.sheet.links;
            assert!(exists(l.line) && exists(l.fill) && exists(l.text), "case {case}: style {} {l:?}", style.id);
        }
        // And it is the same master the next time.
        let again = bring_master(&mut doc, &from, 0, BOTH).unwrap();
        assert_eq!((again.id, again.added), (brought.id, false), "case {case}");
    }
}

// ------------------------------------------------------------------------------------------------------------- the work a copy does

use std::time::Instant;

use super::{COLOUR_ENTRIES_EXAMINED, SHEET_COMPARES, SHEETS_FINGERPRINTED};

fn compares() -> usize {
    SHEET_COMPARES.with(std::cell::Cell::get)
}

/// A document whose styles 1 to `n` are all called "Same", style k based on k + 1 (the last on the root), each with a sheet of its own. A master of
/// `with_master` documents is linked to style 1.
fn alike_styles(n: u32, with_master: bool) -> Document {
    let mut doc = Document::new_blank();
    for k in 1..=n {
        let next = if k == n { 0 } else { k + 1 };
        let mut sheet = Sheet { links: Links { line: Some(next), fill: Some(next), text: Some(next) }, ..Sheet::default() };
        sheet.cells.insert(CellName::new("LineWeight"), cell(&k.to_string()));
        doc.add_style(Style { id: k, name: Some(Arc::from("Same")), name_u: Some(Arc::from("Same")), sheet: Arc::new(sheet), ..Style::default() })
            .unwrap();
    }
    if with_master {
        let mut m = master(1, "Linked", Some(A), Some(B));
        let mut shape = Shape::new(5);
        shape.sheet = Arc::new(Sheet { links: Links { line: Some(1), fill: Some(1), text: Some(1) }, ..Sheet::default() });
        m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        doc.add_master(m).unwrap();
    }
    doc
}

#[test]
fn a_chain_of_styles_with_as_many_alike_named_in_the_drawing_is_one_comparison_each() {
    // The drawing has every style the chain has, under the same names and ids: each style of the chain is found. A search of the drawing's styles
    // of the name for each would compare n * n / 2 sheets (4.5 million here); by hash it is one comparison each.
    let n = 3_000;
    let from = alike_styles(n, true);
    let mut doc = alike_styles(n, false);
    SHEET_COMPARES.with(|c| c.set(0));
    let started = Instant::now();
    let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap();
    println!("{n} styles against {n} of the name: {:?}, {} sheets compared", started.elapsed(), compares());
    assert_eq!((brought.added, brought.styles_added), (true, 0), "every style of the chain was found");
    assert!(compares() <= 2 * n as usize, "{} comparisons for a chain of {n}", compares());
}

#[test]
fn a_drawing_of_alike_named_styles_that_differ_costs_the_chain_a_hash_each_and_no_comparison() {
    // The drawing's 6,000 styles of the name differ from the chain's 6,000 (another weight), so none is found: every one is added, and none of the
    // drawing's is compared with, since none hashes as it does. Counted for n and 2n (the sheets fingerprinted: the drawing's once, the chain's
    // once), the work should double and not quadruple.
    let count = |n: u32| {
        let from = alike_styles(n, true);
        let mut doc = Document::new_blank();
        for k in 1..=n {
            let mut sheet = Sheet::default();
            sheet.cells.insert(CellName::new("LineWeight"), cell(&format!("w{k}")));
            doc.add_style(Style {
                id: k,
                name: Some(Arc::from("Same")),
                name_u: Some(Arc::from("Same")),
                sheet: Arc::new(sheet),
                ..Style::default()
            })
            .unwrap();
        }
        SHEET_COMPARES.with(|c| c.set(0));
        SHEETS_FINGERPRINTED.with(|c| c.set(0));
        let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap();
        assert_eq!(brought.styles_added, n as usize);
        assert_eq!(compares(), 0, "nothing the drawing has hashes as the chain's does");
        SHEETS_FINGERPRINTED.with(std::cell::Cell::get)
    };
    let (small, large) = (count(6_000), count(12_000));
    println!("6,000 styles: {small} sheets fingerprinted, 12,000: {large}");
    assert_eq!(
        (small, large),
        (12_000, 24_000),
        "a hash for each style of the drawing and of the chain, and no more: twice the styles, twice the work"
    );
}

#[test]
fn a_great_many_colour_numbers_cost_the_two_tables_once_and_not_a_scan_each() {
    // The source's table holds 4,096 colours and the master names 300,000 different numbers, nearly all of which name nothing; the drawing's table is the
    // same 4,096. A scan of the source's table for each number is 1.2 billion comparisons; by index it is the tables once and a lookup each.
    let mut from = Document::new_blank();
    let mut doc = Document::new_blank();
    for k in 0..4_096_u32 {
        Arc::make_mut(&mut from.colors).push(ColorEntry { ix: 24 + k, rgb: Arc::from(format!("#{:06X}", k + 1).as_str()) });
        Arc::make_mut(&mut doc.colors).push(ColorEntry { ix: 24 + k, rgb: Arc::from(format!("#{:06X}", k + 1).as_str()) });
    }
    let mut m = master(1, "Many", Some(A), Some(B));
    let mut list = Vec::new();
    for part in 0..3_u32 {
        let mut section = Section::new("Character", None);
        for k in 0..100_000_u32 {
            let mut row = Row { ix: Some(k), ..Row::default() };
            row.cells.insert(CellName::new("Color"), cell(&(24 + part * 100_000 + k).to_string()));
            section.rows.push(Arc::new(row));
        }
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(section));
        let mut shape = Shape::new(5 + part);
        shape.sheet = Arc::new(sheet);
        list.push(Arc::new(shape));
    }
    m.shapes = ShapeList::from_vec(list);
    from.add_master(m).unwrap();
    COLOUR_ENTRIES_EXAMINED.with(|c| c.set(0));
    let started = Instant::now();
    let brought = bring_master(&mut doc, &from, 0, BOTH);
    let took = started.elapsed();
    println!("300,000 colour numbers: {took:?}, {} table entries examined", COLOUR_ENTRIES_EXAMINED.with(std::cell::Cell::get));
    // The drawing's table is the source's, so every colour is found where it is and nothing is added.
    assert_eq!(brought.map(|b| (b.added, b.colours_added)), Ok((true, 0)));
    assert_eq!(COLOUR_ENTRIES_EXAMINED.with(std::cell::Cell::get), 8_192, "each table once");
    // A hang guard and no measure of cost (the count above is the claim; the copy takes a second or two at load 50).
    assert!(took < std::time::Duration::from_secs(60), "{took:?}");
}

// ------------------------------------------------------------------------------------------------------ what a copy keeps of the stencil's text

fn with_kept_element(element: diacraft_doc::RawElement) -> diacraft_doc::Passthrough {
    let mut kept = diacraft_doc::Passthrough::new();
    kept.push(0, diacraft_doc::RawNode::Element(element));
    kept
}

fn relationship_element(prefix_declared_inside: bool) -> diacraft_doc::RawElement {
    let mut attrs = Vec::new();
    if prefix_declared_inside {
        attrs.push(("xmlns:q", super::refs::NS_REL));
    }
    attrs.push(("q:id", "rId1"));
    diacraft_doc::RawElement { name: Arc::from("Thing"), attrs: RawAttrs::from_pairs(attrs), children: Vec::new() }
}

#[test]
fn a_relationship_kept_in_a_master_row_a_style_or_a_cell_refuses_the_master() {
    let from_with = |edit: &dyn Fn(&mut Document)| {
        let mut from = source_with_parts();
        edit(&mut from);
        from
    };
    let cases: Vec<(&str, Document)> = vec![
        (
            "the master row's own element",
            from_with(&|d| {
                let m = Arc::make_mut(d.masters.get_mut(0).unwrap());
                m.passthrough = with_kept_element(relationship_element(true));
            }),
        ),
        (
            "the master row's own attribute",
            from_with(&|d| {
                let m = Arc::make_mut(d.masters.get_mut(0).unwrap());
                m.raw.push("xmlns:q", super::refs::NS_REL);
                m.raw.push("q:id", "rId1");
            }),
        ),
        (
            "a style's element",
            from_with(&|d| {
                let at = d.styles.iter().position(|s| s.id == 3).unwrap();
                Arc::make_mut(d.styles.get_mut(at).unwrap()).passthrough = with_kept_element(relationship_element(true));
            }),
        ),
        (
            "a style's attribute, bound above (out of sight)",
            from_with(&|d| {
                let at = d.styles.iter().position(|s| s.id == 4).unwrap();
                Arc::make_mut(d.styles.get_mut(at).unwrap()).raw.push("rel:id", "rId1");
            }),
        ),
        (
            "a cell's kept element",
            from_with(&|d| {
                let m = Arc::make_mut(d.masters.get_mut(0).unwrap());
                let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
                let sheet = Arc::make_mut(&mut shape.sheet);
                let cell = sheet.cells.get_mut("LineColor").unwrap();
                cell.extra = Some(Arc::new(diacraft_doc::CellExtra {
                    text: None,
                    passthrough: with_kept_element(relationship_element(true)),
                    ..Default::default()
                }));
            }),
        ),
    ];
    for (what, from) in cases {
        let mut doc = Document::new_blank();
        let before = doc.clone();
        assert_eq!(bring_master(&mut doc, &from, 0, BOTH), Err(Refusal::Pictures(1)), "{what}");
        assert_eq!(doc, before, "{what}");
    }
}

#[test]
fn the_face_cap_is_on_what_a_copy_adds() {
    // A drawing with more faces than the cap can still take a master that names none of them, or only faces it has; one more face past the cap is refused.
    let mut doc = Document::new_blank();
    for k in 0..=super::MAX_FACES {
        Arc::make_mut(&mut doc.fonts).push(FaceName {
            name_u: Arc::from(format!("Face {k}").as_str()),
            raw: RawAttrs::new(),
            ext: Default::default(),
        });
    }
    let mut from = source_with_parts();
    // The master names "Face 7", which the drawing has: nothing is added.
    let m = Arc::make_mut(from.masters.get_mut(0).unwrap());
    let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
    let row = Arc::make_mut(section.rows.get_mut(0).unwrap());
    row.cells.insert(CellName::new("Font"), cell("Face 7"));
    assert!(bring_master(&mut doc, &from, 0, BOTH).is_ok(), "no face is added");
    // A face the drawing lacks would be the one past the cap.
    let mut other = doc.clone();
    let row_name = "Segoe Script";
    let m = Arc::make_mut(from.masters.get_mut(0).unwrap());
    let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
    let row = Arc::make_mut(section.rows.get_mut(0).unwrap());
    row.cells.insert(CellName::new("Font"), cell(row_name));
    Arc::make_mut(&mut from.fonts).push(FaceName { name_u: Arc::from(row_name), raw: RawAttrs::new(), ext: Default::default() });
    other.masters = diacraft_doc::SharedVec::new();
    assert_eq!(bring_master(&mut other, &from, 0, BOTH), Err(Refusal::Limit("face names")));
}

#[test]
fn a_copy_keeps_no_text_of_the_stencils_ids_and_no_master_a_master_shape_names() {
    // The reader keeps `ID="05"`, `LineStyle="03"` and (in a master part, where they are not read as fields) `Master` and `MasterShape` as text in
    // `raw`, which wins on a write; a copy that kept them would be saved under the stencil's numbers.
    let mut from = Document::new_blank();
    let mut base = Sheet::default();
    base.cells.insert(CellName::new("LineWeight"), cell("0.02"));
    let mut style_raw = RawAttrs::new();
    style_raw.push("ID", "04");
    style_raw.push("IsCustomName", "1");
    from.add_style(Style {
        id: 4,
        name: Some(Arc::from("Base")),
        name_u: Some(Arc::from("Base")),
        sheet: Arc::new(base),
        raw: style_raw,
        ..Style::default()
    })
    .unwrap();
    let mut m = master(5, "Padded", Some(A), Some(B));
    m.raw.push("ID", "05");
    let mut shape = Shape::new(5);
    let mut sheet = Sheet { links: Links { line: Some(4), fill: None, text: None }, ..Sheet::default() };
    sheet.raw.push("LineStyle", "04");
    sheet.raw.push("OtherAttribute", "kept");
    shape.sheet = Arc::new(sheet);
    shape.raw.push("Master", "9");
    shape.raw.push("MasterShape", "9");
    shape.raw.push("OriginalID", "7");
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    // The drawing has style 4 as another style, so the link must change, and a master 5.
    let mut doc = Document::new_blank();
    doc.add_style(Style { id: 4, name: Some(Arc::from("Mine")), ..Style::default() }).unwrap();
    doc.add_master(master(5, "Five", Some(C), Some(C))).unwrap();
    let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap();
    assert_eq!(brought.id, 6);
    let copy = doc.master(6).unwrap();
    assert_eq!(copy.raw.get("ID"), None, "the master is saved under its new id");
    let shape = copy.shapes.get(0).unwrap();
    assert_eq!((shape.raw.get("Master"), shape.raw.get("MasterShape")), (None, None));
    assert_eq!((shape.master, shape.master_shape), (None, None));
    assert_eq!(shape.raw.get("OriginalID"), Some("7"), "what else the shape kept stays");
    let new_base = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Base")).unwrap();
    assert_ne!(new_base.id, 4);
    assert_eq!(new_base.raw.get("ID"), None, "the style is saved under its new id");
    assert_eq!(new_base.raw.get("IsCustomName"), Some("1"));
    assert_eq!(shape.sheet.links.line, Some(new_base.id));
    assert_eq!(shape.sheet.raw.get("LineStyle"), None, "the old link's text would win on a write");
    assert_eq!(shape.sheet.raw.get("OtherAttribute"), Some("kept"));
    // The source is as it was.
    let was = from.masters.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!(
        (was.raw.get("Master"), was.sheet.raw.get("LineStyle"), from.masters.get(0).unwrap().raw.get("ID")),
        (Some("9"), Some("04"), Some("05"))
    );
}

#[test]
fn a_shape_that_names_a_master_is_copied_even_when_no_link_or_colour_changes() {
    let mut from = Document::new_blank();
    let mut m = master(1, "Names", Some(A), Some(B));
    let mut shape = Shape::new(5);
    shape.raw.push("Master", "9");
    m.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    from.add_master(m).unwrap();
    let mut doc = Document::new_blank();
    let brought = bring_master(&mut doc, &from, 0, BOTH).unwrap();
    assert_eq!(doc.master(brought.id).unwrap().shapes.get(0).unwrap().raw.get("Master"), None);
}

mod kept;

#[test]
fn a_master_brought_from_a_file_leaves_the_name_of_its_part_and_the_rel_of_its_row_behind() {
    // A master read from a stencil file knows the part it came from and what that part's root and its row's `Rel` held. The copy is the drawing's: two
    // masters from two stencils would otherwise name one part of the drawing's package.
    let mut from = Document::new_blank();
    let mut m = master(1, "Box", Some(A), None);
    m.part_name = Some(Arc::from("/visio/masters/master1.xml"));
    let kept = |attr: (&str, &str)| diacraft_doc::Kept { raw: RawAttrs::from_pairs([attr]), ..Default::default() };
    m.ext.make_mut().vsdx.0.push((Arc::from("Rel"), kept(("k", "v"))));
    m.ext.make_mut().vsdx.0.push((Arc::from("MasterContents"), kept(("xml:space", "preserve"))));
    m.ext.make_mut().vsdx.0.push((Arc::from("Shapes"), kept(("x", "1"))));
    m.ext.make_mut().vsdx.0.push((Arc::from("Connects"), kept(("y", "2"))));
    m.ext.make_mut().vsdx.0.push((Arc::from("NoPageSheet"), diacraft_doc::Kept::default()));
    from.add_master(m).unwrap();
    let mut doc = Document::new_blank();
    let brought = bring_master(&mut doc, &from, 0, Identity::within(&from, 0)).unwrap();
    let copy = doc.master(brought.id).unwrap();
    assert_eq!(copy.part_name, None);
    // What names a place in the other package or spells it goes (the row's `Rel`, the part's root, here no prefix); what is the master's content (its `Shapes` and `Connects`, and whether
    // its row had a `PageSheet`) stays.
    let keys: Vec<&str> = copy.ext.get().map(|e| e.vsdx.0.iter().map(|(k, _)| &**k).collect()).unwrap_or_default();
    assert_eq!(keys, ["Shapes", "Connects", "NoPageSheet"]);
    // The source keeps both.
    assert!(from.master(1).unwrap().part_name.is_some());
}
