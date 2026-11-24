//! What the path functions and the text measures read, and the edits that work them out again.
//!
//! A path function reads every row of the section its first argument names, and the `Width`, `Height` and `Rounding` of the shape; a text
//! measure reads the formats of the runs, the tab stops, the margins and the page's scale. None of these is named by the formula, so the
//! dependency graph links them. The cases that matter: **an edit nothing links leaves a saved result as the file saved it** (a curve's length
//! is saved a little off, and a commit that rewrote it on every edit would rewrite what was built not to), and an edit that is
//! linked works the cell out again.

use diacraft_calc::{Budget, EvalOptions, Fonts, changes_between, commit_changed};
use diacraft_doc::{CellAddr, CellEdit, Document, Lookup, Owner, RowKey, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use crate::path_functions::{earc, ln, mv, section};
use crate::text_measure::{TERM, em, parts, pt, shape};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn sheet_of(doc: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(doc.find_shape(Owner::Page(1), id).expect("a shape"))
}

fn write(doc: &mut Document, sheet: &SheetRef, addr: &CellAddr, value: &str) {
    doc.write_cell(sheet, addr, CellEdit::Value(DocValue::from_text(value)), WriteOrigin::Drawing).expect("a write");
}

/// The saved `V` text of a user cell of a shape.
fn saved(doc: &Document, id: u32, name: &str) -> String {
    let at = doc.find_shape(Owner::Page(1), id).expect("a shape");
    match doc.shape(&at).expect("a shape").sheet.lookup(&user(name)) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default(),
        other => panic!("{other:?}"),
    }
}

fn commit(doc: &mut Document, before: &Document, options: EvalOptions) -> diacraft_calc::CommitReport {
    let changes = changes_between(before, doc, 1);
    commit_changed(doc, &Budget::default(), options, 1, &changes)
}

/// The length of the curve of a move to (0, 0) and an elliptical arc to (4, 0) through (2, 1) with a ratio of 2, 4.8442 and some.
const CURVE: f64 = 4.844_2;

fn curve_shape(id: u32, length: &str) -> ShapeBuilder {
    section(ShapeBuilder::new(id), 0, vec![mv(0.0, 0.0), earc(4.0, 0.0, 2.0, 1.0, 0.0, 2.0)])
        .row_formula("User", None, RowKey::name("Len"), "Value", "PATHLENGTH(Geometry1.Path)", length)
        .row_formula("User", None, RowKey::name("Near"), "Value", "NEARESTPOINTONPATH(Geometry1.Path,1,1)", "0.3")
}

/// The saved text of a length a little off the true one, as the application saved curves (2E-5 off).
fn off(true_value: f64) -> String {
    (true_value * (1.0 + 1.5e-5)).to_string()
}

/// A page of the curve shape (1), a second shape that reads its path by name (2), and a shape with a second section (3). Every saved length is a
/// little off, so a commit that works a cell out writes it, and one that leaves it alone does not.
fn curves() -> Document {
    let diacraft_calc::Value::Number(q) =
        crate::eval_on(|s| section(s, 0, vec![mv(0.0, 0.0), earc(4.0, 0.0, 2.0, 1.0, 0.0, 2.0)]), "PATHLENGTH(Geometry1.Path)")
    else {
        panic!("not a number")
    };
    let truth = q.stored;
    assert!((truth - CURVE).abs() < 1e-3, "{truth}");
    let one = curve_shape(1, &off(truth));
    let two = ShapeBuilder::new(2).row_formula("User", None, RowKey::name("Other"), "Value", "PATHLENGTH(Sheet.1!Geometry1.Path)", &off(truth));
    let three = section(ShapeBuilder::new(3), 0, vec![mv(0.0, 0.0), ln(3.0, 0.0)]);
    let three = section(three, 1, vec![mv(0.0, 0.0), ln(0.0, 5.0)])
        .row_formula("User", None, RowKey::name("First"), "Value", "PATHLENGTH(Geometry1.Path)", "3.00002")
        .row_formula("User", None, RowKey::name("Second"), "Value", "PATHLENGTH(Geometry2.Path)", "5.00002");
    DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(two).shape(three)).build()
}

/// An edit that nothing a path function reads is left alone: moving the shape, turning it, writing another cell, editing the other section.
/// The saved lengths stay as the file saved them, byte for byte, where a commit that worked every path function out after every edit rewrote them
/// (they are 1.5E-5 off).
#[test]
fn an_unrelated_edit_leaves_a_curved_path_length_as_it_was_saved() {
    let before = curves();
    let (len, near, other) = (saved(&before, 1, "Len"), saved(&before, 1, "Near"), saved(&before, 2, "Other"));
    let mut doc = before.clone();
    let one = sheet_of(&doc, 1);
    for (name, value) in [("PinX", "7"), ("PinY", "2"), ("Angle", "0.5"), ("LocPinX", "1"), ("FlipX", "1")] {
        write(&mut doc, &one, &CellAddr::direct(name), value);
    }
    write(&mut doc, &one, &user("Elsewhere"), "3");
    let report = commit(&mut doc, &before, EvalOptions::default());
    assert_eq!(report.written, 0, "{report:?}");
    assert_eq!((saved(&doc, 1, "Len"), saved(&doc, 1, "Near"), saved(&doc, 2, "Other")), (len.clone(), near.clone(), other.clone()));
    // A move of the shape that is read by name is no edit of its path either.
    let mut moved = before.clone();
    write(&mut moved, &one, &CellAddr::direct("PinX"), "99");
    assert_eq!(commit(&mut moved, &before, EvalOptions::default()).written, 0);
    assert_eq!(saved(&moved, 2, "Other"), other);
    // An edit to the other section of another shape reaches the cell that reads that section and not the one that reads the first.
    let mut third = before.clone();
    let three = sheet_of(&third, 3);
    write(&mut third, &three, &CellAddr::in_row("Geometry", Some(1), RowKey::Ix(1), "Y"), "8");
    let report = commit(&mut third, &before, EvalOptions::default());
    assert_eq!(saved(&third, 3, "Second"), "8", "{report:?}");
    assert_eq!(saved(&third, 3, "First"), "3.00002", "the first section is not the one that changed");
    assert_eq!((saved(&third, 1, "Len"), saved(&third, 2, "Other")), (len, other));
}

/// An edit of what a path function reads works it out again: a cell of a row of its section, the size of the shape, its rounding; and a cell that
/// reads the path of another shape follows that shape's rows. The cell takes the true value, which is not the saved one.
#[test]
fn an_edit_of_what_a_path_reads_works_it_out_again() {
    let before = curves();
    // What the evaluator gives the cell `Len` of shape 1 on the document as edited: the true length, to compare the saved one with.
    let truth = |doc: &Document| -> f64 {
        let at = diacraft_calc::CellAddress::new(sheet_of(doc, 1), user("Len"));
        let diacraft_calc::Value::Number(q) = diacraft_calc::evaluate(doc, &at, &mut Default::default()) else { panic!("not a number") };
        q.stored
    };
    let rows: [(&str, CellAddr, &str); 4] = [
        ("a row of the section", CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), "6"),
        ("the size", CellAddr::direct("Width"), "3"),
        ("the height", CellAddr::direct("Height"), "3"),
        ("the rounding", CellAddr::direct("Rounding"), "0.25"),
    ];
    for (what, addr, value) in rows {
        let mut doc = before.clone();
        let one = sheet_of(&doc, 1);
        write(&mut doc, &one, &addr, value);
        let report = commit(&mut doc, &before, EvalOptions::default());
        assert!(report.written >= 2, "{what}: {report:?}");
        let (len, other) = (saved(&doc, 1, "Len").parse::<f64>().unwrap(), saved(&doc, 2, "Other").parse::<f64>().unwrap());
        let want = truth(&doc);
        assert!((len - want).abs() < 1e-9 && (other - want).abs() < 1e-9, "{what}: {len} and {other} against {want}");
    }
    // The row edit really moved the length.
    let mut doc = before.clone();
    let one = sheet_of(&doc, 1);
    write(&mut doc, &one, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), "6");
    commit(&mut doc, &before, EvalOptions::default());
    assert!(saved(&doc, 1, "Len").parse::<f64>().unwrap() > CURVE + 1.0);
}

/// An edit nothing links leaves a measure alone, and an edit of what it reads works it out again: the size of a run (`Character`), a margin, the
/// tab stops, and the scale of the page. The saved measure is a stale 9, so a commit that worked it out shows.
#[test]
fn a_text_measure_follows_the_formats_margins_and_scale_it_reads() {
    let options = || EvalOptions { fonts: Some(em()), ..EvalOptions::default() };
    let build = || {
        let s = shape("abc\n", 12.0)
            .row_formula("User", None, RowKey::name("W"), "Value", "TEXTWIDTH(TheText)", "9")
            .row_formula("User", None, RowKey::name("H"), "Value", "TEXTHEIGHT(TheText,4 in)", "9")
            .row_cell("Tabs", None, RowKey::Ix(0), "Position0", "0.5");
        let other = ShapeBuilder::new(2).row_formula("User", None, RowKey::name("W"), "Value", "TEXTWIDTH(Sheet.1!TheText)", "9");
        DocBuilder::new().page(1, PageBuilder::new().cell("PageScale", "1").cell("DrawingScale", "1").shape(s).shape(other)).build()
    };
    let before = build();
    // Nothing the measure reads: a move, a turn, another cell.
    let mut doc = before.clone();
    let one = sheet_of(&doc, 1);
    for (name, value) in [("PinX", "7"), ("Angle", "0.5"), ("Width", "3")] {
        write(&mut doc, &one, &CellAddr::direct(name), value);
    }
    assert_eq!(commit(&mut doc, &before, options()).written, 0);
    assert_eq!((saved(&doc, 1, "W"), saved(&doc, 1, "H"), saved(&doc, 2, "W")), ("9".into(), "9".into(), "9".into()));
    // What it reads.
    let ten = pt(10.0).to_string();
    let cases: Vec<(&str, SheetRef, CellAddr, &str)> = vec![
        ("a run's size", one.clone(), CellAddr::in_row("Character", None, RowKey::Ix(0), "Size"), &ten),
        ("a run's font scale", one.clone(), CellAddr::in_row("Character", None, RowKey::Ix(0), "FontScale"), "2"),
        ("a paragraph's spacing", one.clone(), CellAddr::in_row("Paragraph", None, RowKey::Ix(0), "SpLine"), "-2"),
        ("a margin", one.clone(), CellAddr::direct("LeftMargin"), "0.5"),
        ("a tab stop", one.clone(), CellAddr::in_row("Tabs", None, RowKey::Ix(0), "Position0"), "1"),
        ("the page's scale", SheetRef::Page(1), CellAddr::direct("PageScale"), "0.5"),
    ];
    for (what, sheet, addr, value) in cases {
        let mut doc = before.clone();
        write(&mut doc, &sheet, &addr, value);
        let report = commit(&mut doc, &before, options());
        assert!(report.written >= 1, "{what}: {report:?}");
        assert_ne!(saved(&doc, 1, "W"), "9", "{what}: the width of the shape's own text is worked out again");
        assert_ne!(saved(&doc, 2, "W"), "9", "{what}: so is the width of its text read from another shape");
    }
    // The height is not a width: a margin to the left leaves it where it is, a size or a line spacing moves it.
    let mut doc = before.clone();
    write(&mut doc, &one, &CellAddr::direct("LeftMargin"), "0.5");
    commit(&mut doc, &before, options());
    assert_ne!(saved(&doc, 1, "W"), "9");
    assert!((saved(&doc, 1, "H").parse::<f64>().unwrap() - (0.2 + 8.0 / 72.0 + TERM)).abs() < 1e-9, "the height is worked out too, and is the same");
}

/// An instance whose master holds the formula: the cell is `Inh` on the instance, and a size the instance changes works it out again.
#[test]
fn an_instance_follows_what_its_masters_measure_reads() {
    let options = || EvalOptions { fonts: Some(em()), ..EvalOptions::default() };
    let master = ShapeBuilder::new(5).formula("TxtWidth", "TEXTWIDTH(TheText)", "1");
    let instance = ShapeBuilder::new(1)
        .master(1)
        .inh("TxtWidth", Some("9"))
        .text_parts(parts("abc\n"))
        .row_cell("Character", None, RowKey::Ix(0), "Font", "Em")
        .row_cell("Character", None, RowKey::Ix(0), "Size", &pt(12.0).to_string())
        .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "-1.2");
    let before = DocBuilder::new().master(1, MasterBuilder::new().shape(master)).page(1, PageBuilder::new().shape(instance)).build();
    let text_width = |doc: &Document| match doc
        .shape(&doc.find_shape(Owner::Page(1), 1).expect("a shape"))
        .expect("a shape")
        .sheet
        .lookup(&CellAddr::direct("TxtWidth"))
    {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default(),
        other => panic!("{other:?}"),
    };
    let mut quiet = before.clone();
    let one = sheet_of(&quiet, 1);
    write(&mut quiet, &one, &CellAddr::direct("PinX"), "3");
    commit(&mut quiet, &before, options());
    assert_eq!(text_width(&quiet), "9", "an edit nothing links leaves the cell");
    let mut doc = before.clone();
    write(&mut doc, &one, &CellAddr::in_row("Character", None, RowKey::Ix(0), "Size"), &pt(24.0).to_string());
    commit(&mut doc, &before, options());
    let width: f64 = text_width(&doc).parse().unwrap();
    assert!((width - (4.0 / 3.0 + TERM)).abs() < 1e-6, "four em of 24 points: {width}");
}

/// The fonts a commit measures in are the ones it is given: the same edit under another database writes another width.
#[test]
fn a_commit_measures_in_the_faces_it_is_given() {
    let build = || {
        let s = shape("abc\n", 12.0).row_formula("User", None, RowKey::name("W"), "Value", "TEXTWIDTH(TheText)", "9");
        DocBuilder::new().page(1, PageBuilder::new().shape(s)).build()
    };
    let before = build();
    let narrow = Fonts::new({
        let mut db = diacraft_fonts::FontDb::empty();
        db.add_font_data(diacraft_fonts::synth::SynthFont::new("Em").ascii(500).build());
        db
    });
    let mut a = before.clone();
    let mut b = before.clone();
    let one = sheet_of(&a, 1);
    write(&mut a, &one, &CellAddr::in_row("Character", None, RowKey::Ix(0), "Size"), &pt(14.0).to_string());
    write(&mut b, &one, &CellAddr::in_row("Character", None, RowKey::Ix(0), "Size"), &pt(14.0).to_string());
    commit(&mut a, &before, EvalOptions { fonts: Some(em()), ..EvalOptions::default() });
    commit(&mut b, &before, EvalOptions { fonts: Some(narrow), ..EvalOptions::default() });
    let (wide, thin) = (saved(&a, 1, "W").parse::<f64>().unwrap(), saved(&b, 1, "W").parse::<f64>().unwrap());
    assert!(wide > thin + 0.1, "{wide} against {thin}");
}
