//! The formatting commands read a cell that holds a formula and no saved value as the picture draws it, and not as a cell that holds nothing: the
//! planners that decide what to write (Quick Styles, fill and line, the text of a run) ask what a cell holds, and a cell the file saved no value for holds what its
//! formula says.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, Shape, Sheet, Value};

use super::{Paint, QuickStyle, Rgb, ShapeFormat, TextFormat, format_quick_style, format_shapes, format_text};
use crate::draw::page_mut;
use crate::theme::{Base, Choice};

/// A drawing with one shape, 1, that follows the `Theme` style of the page's theme. With `unsaved` the cells that hold the marker hold the call and no value (the file left
/// the value out), in the styles and in the shape: the same drawing, as far as what is drawn goes.
fn themed_shape(unsaved: bool) -> Document {
    let mut doc = Document::new_blank();
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(Shape::new(1)));
    let theme = crate::theme::find("tidepool").unwrap();
    crate::theme::apply(&mut doc, &[1], Choice::Theme(theme, 0), Base::Root).unwrap();
    if unsaved {
        leave_the_markers_out(&mut doc);
    }
    doc
}

/// Takes the value off every cell that holds the marker `Themed`, in every style and every shape of the first page, directly under a sheet and in rows.
fn leave_the_markers_out(doc: &mut Document) {
    fn strip(sheet: &mut Sheet) {
        for cell in sheet.cells.values_mut() {
            if cell.value.as_ref().is_some_and(Value::is_themed) {
                cell.value = None;
            }
        }
        for section in &mut sheet.sections {
            for row in Arc::make_mut(section).rows.iter_mut() {
                for cell in Arc::make_mut(row).cells.values_mut() {
                    if cell.value.as_ref().is_some_and(Value::is_themed) {
                        cell.value = None;
                    }
                }
            }
        }
    }
    for at in 0..doc.styles.len() {
        if let Some(style) = doc.styles.get_mut(at) {
            strip(Arc::make_mut(&mut Arc::make_mut(style).sheet));
        }
    }
    for at in 0..doc.pages.len() {
        let Some(page) = doc.pages.get_mut(at) else { continue };
        let page = Arc::make_mut(page);
        for index in 0..page.shapes.len() {
            if let Some(shape) = page.shapes.get_mut(index) {
                strip(Arc::make_mut(&mut Arc::make_mut(shape).sheet));
            }
        }
    }
}

fn shape_of(doc: &Document) -> &Shape {
    doc.shape(&doc.find_shape(Owner::Page(1), 1).unwrap()).unwrap()
}

/// What the shape holds itself: the names of its cells directly under the sheet and the number of rows of each section.
fn local(doc: &Document) -> (Vec<String>, Vec<usize>) {
    let sheet = &shape_of(doc).sheet;
    (sheet.cells.keys().map(|n| n.as_str().to_owned()).collect(), sheet.sections.iter().map(|s| s.rows.len()).collect())
}

fn cell_of(doc: &Document, name: &str) -> Cell {
    shape_of(doc).sheet.cell(name).cloned().unwrap_or_else(|| panic!("the shape holds no {name}"))
}

fn set(doc: &mut Document, name: &str, cell: Cell) {
    let shape = Arc::make_mut(page_mut(doc, 1).unwrap().shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet).set_cell(&CellAddr::direct(name), cell).unwrap();
}

fn formula_text(cell: &Cell) -> Option<&str> {
    cell.formula.text()
}

#[test]
fn a_quick_style_does_not_write_what_a_shape_reads_from_the_theme_style_whether_or_not_the_style_saved_a_value() {
    // The cells of line, fill and shadow that the shape reads from the `Theme` style are the theme's already, and are not written: the style saved the marker, or only the
    // call. Counted in what the shape ends up holding: the same cells whichever way the file left the style.
    let style = QuickStyle::at(2, 3).unwrap();
    let (mut saved, mut unsaved) = (themed_shape(false), themed_shape(true));
    format_quick_style(&mut saved, 1, &[1], style).unwrap();
    format_quick_style(&mut unsaved, 1, &[1], style).unwrap();
    assert_eq!(local(&unsaved), local(&saved), "the same cells written to the shape");
}

#[test]
fn a_quick_style_leaves_scheme_cells_that_work_out_to_the_page_alone() {
    // `ThemeIndex` holds a formula and no value, and works out to 65534: the shape already reads the page's theme, and the cell is not written over.
    let mut doc = themed_shape(true);
    set(&mut doc, "ThemeIndex", Cell::with_formula("65530+4", None));
    format_quick_style(&mut doc, 1, &[1], QuickStyle::at(1, 2).unwrap()).unwrap();
    let cell = cell_of(&doc, "ThemeIndex");
    assert_eq!(formula_text(&cell), Some("65530+4"), "the formula stands: {cell:?}");
}

fn solid(colour: (u8, u8, u8)) -> ShapeFormat {
    ShapeFormat { fill: Some(Paint::Colour(Rgb(colour.0, colour.1, colour.2))), ..ShapeFormat::default() }
}

/// A shape of a blank drawing with a pattern and a gradient flag that hold formulas and no values.
fn with_unsaved_fill(pattern: &str, gradient: &str) -> Document {
    let mut doc = Document::new_blank();
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(Shape::new(1)));
    set(&mut doc, "FillPattern", Cell::with_formula(pattern, None));
    set(&mut doc, "FillGradientEnabled", Cell::with_formula(gradient, None));
    doc
}

#[test]
fn a_solid_colour_turns_off_a_gradient_that_a_formula_with_no_saved_value_turns_on() {
    // The picture draws the gradient (the flag works out to 1), so a colour chosen for the fill must turn it off, as it does for a flag that was saved as 1.
    let mut doc = with_unsaved_fill("1", "0+1");
    format_shapes(&mut doc, 1, &[1], &solid((10, 20, 30))).unwrap();
    let flag = cell_of(&doc, "FillGradientEnabled");
    assert!(formula_text(&flag).is_none() && flag.value.as_ref().map(Value::text) == Some("0"), "the flag is turned off: {flag:?}");
    // A flag that works out to 0 is left as it is.
    let mut doc = with_unsaved_fill("1", "1-1");
    format_shapes(&mut doc, 1, &[1], &solid((10, 20, 30))).unwrap();
    assert_eq!(formula_text(&cell_of(&doc, "FillGradientEnabled")), Some("1-1"), "a gradient that is off is not written");
}

#[test]
fn a_solid_colour_keeps_a_pattern_that_a_formula_with_no_saved_value_works_out_to() {
    // Pattern 2 is a pattern a colour shows under; read as holding nothing it was replaced by a solid fill, 1.
    let mut doc = with_unsaved_fill("1+1", "0");
    format_shapes(&mut doc, 1, &[1], &solid((10, 20, 30))).unwrap();
    assert_eq!(formula_text(&cell_of(&doc, "FillPattern")), Some("1+1"), "the pattern stands");
    // One that works out to a pattern no fill has (0 is none) is made solid, as one saved as 0 is.
    let mut doc = with_unsaved_fill("1-1", "0");
    format_shapes(&mut doc, 1, &[1], &solid((10, 20, 30))).unwrap();
    let pattern = cell_of(&doc, "FillPattern");
    assert!(formula_text(&pattern).is_none() && pattern.value.as_ref().map(Value::text) == Some("1"), "{pattern:?}");
}

#[test]
fn a_fill_transparency_reads_the_pattern_and_the_gradient_a_formula_gives() {
    // 24 is a pattern that has a background: its transparency is written too. Worked out from a formula with no value, the pattern is 24; read as nothing it was 1.
    let mut doc = with_unsaved_fill("12*2", "0");
    let format = ShapeFormat { fill_transparency: Some(50.0), ..ShapeFormat::default() };
    format_shapes(&mut doc, 1, &[1], &format).unwrap();
    assert!(shape_of(&doc).sheet.cell("FillBkgndTrans").is_some(), "the background of pattern 24 is made as clear as the colour");
}

#[test]
fn a_run_whose_style_is_a_formula_with_no_saved_value_keeps_what_it_works_out_to_when_bold_is_added() {
    // `Style` works out to 4, underline. Adding bold is 5, and not 1: the cell is read as the picture draws it.
    let mut doc = Document::new_blank();
    let mut shape = Shape::new(1);
    let mut sheet = Sheet::default();
    sheet.set_cell(&CellAddr::in_row("Character", None, RowKey::Ix(0), "Style"), Cell::with_formula("2+2", None)).unwrap();
    shape.sheet = Arc::new(sheet);
    shape.text = Some(Arc::new(diacraft_doc::Text {
        parts: vec![diacraft_doc::TextPart::Cp(0), diacraft_doc::TextPart::Chars("abc".into())],
        ..Default::default()
    }));
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(shape));
    format_text(&mut doc, 1, &[1], None, &TextFormat { bold: Some(true), ..TextFormat::default() }).unwrap();
    let style = shape_of(&doc).sheet.section(&diacraft_doc::SectionKey::new("Character", None)).and_then(|s| s.rows.first().cloned()).unwrap();
    let cell = style.cells.get("Style").unwrap();
    assert_eq!(cell.value.as_ref().map(Value::text), Some("5"), "underline and bold: {cell:?}");
    assert!(formula_text(cell).is_none(), "a number, and not a formula over it");
}

#[test]
fn a_run_cell_that_holds_a_formula_and_no_saved_value_is_no_plain_number_and_is_written_over_though_it_agrees() {
    // `Style` works out to 1 (bold) already: adding bold changes no number, and the cell is written all the same, as a number and no longer the formula, because the cell
    // the command leaves must hold what it was asked for and not a formula that happens to agree. That is `plain`: a cell that holds a formula (with a value or none) or the
    // marker is no plain number.
    let mut doc = Document::new_blank();
    let mut shape = Shape::new(1);
    let mut sheet = Sheet::default();
    sheet.set_cell(&CellAddr::in_row("Character", None, RowKey::Ix(0), "Style"), Cell::with_formula("0+1", None)).unwrap();
    shape.sheet = Arc::new(sheet);
    shape.text = Some(Arc::new(diacraft_doc::Text {
        parts: vec![diacraft_doc::TextPart::Cp(0), diacraft_doc::TextPart::Chars("abc".into())],
        ..Default::default()
    }));
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(shape));
    format_text(&mut doc, 1, &[1], None, &TextFormat { bold: Some(true), ..TextFormat::default() }).unwrap();
    let row = shape_of(&doc).sheet.section(&diacraft_doc::SectionKey::new("Character", None)).and_then(|s| s.rows.first().cloned()).unwrap();
    let cell = row.cells.get("Style").unwrap();
    assert_eq!(cell.value.as_ref().map(Value::text), Some("1"), "{cell:?}");
    assert!(formula_text(cell).is_none(), "the formula is replaced by the number: {cell:?}");
}

#[test]
fn the_look_reader_works_a_cell_with_no_saved_value_out_in_steps_of_its_own() {
    // The steps of the themed cells are none here, and the cell is worked out all the same: its steps are its own, as the picture's frame has them.
    let mut doc = Document::new_blank();
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(Shape::new(1)));
    set(&mut doc, "FillForegnd", Cell::with_formula("RGB(1,2,3)", None));
    let mut ctx = diacraft_calc::EvalCtx::new(diacraft_calc::EvalOptions::default(), diacraft_calc::Budget::with_steps(0));
    let reader = super::look::Reader::new(&doc, &mut ctx);
    let sheet = diacraft_doc::SheetRef::Shape(doc.find_shape(Owner::Page(1), 1).unwrap());
    assert_eq!(reader.colour(&sheet, &CellAddr::direct("FillForegnd")), Some(Rgb(1, 2, 3)));
}

// ---- one budget for a command (the security scan's finding) ------------------------------------------------------------------

#[test]
fn the_gallery_works_what_it_reads_out_in_one_budget_for_all_its_tiles_and_not_one_for_each() {
    use super::look::UnsavedBudget;
    use super::quick::quick_tiles_within;

    // A gallery reads the shape once and again for each of its 46 tiles, each in a reader of its own. The steps for the cells with no saved value are the command's: a
    // budget that the readers take from in turn, and that the first exhaustion ends for the rest, which read their cells as they were before (the picture does the same).
    let doc = themed_shape(true);
    let fills = |look: &super::QuickLook| look.variants.iter().chain(&look.styles).map(|tile| tile.fill.map(|c| c.hex())).collect::<Vec<_>>();
    let plenty = UnsavedBudget::new(10_000_000);
    let full = quick_tiles_within(&doc, 1, &[1], &plenty).unwrap();
    let needed = 10_000_000 - plenty.left();
    assert!(needed > 1_000, "the gallery worked cells out: {needed} steps");
    let full = fills(&full);
    assert!(full.iter().collect::<std::collections::HashSet<_>>().len() > 2, "the theme gives the tiles different fills: {full:?}");

    let nothing = UnsavedBudget::new(0);
    let none = fills(&quick_tiles_within(&doc, 1, &[1], &nothing).unwrap());
    assert_eq!(nothing.left(), 0);

    // A quarter of what the gallery needs: spent whole, and not once for each reader.
    let quarter = UnsavedBudget::new(needed / 4);
    let some = fills(&quick_tiles_within(&doc, 1, &[1], &quarter).unwrap());
    assert_eq!(quarter.left(), 0, "the budget was spent: it is one for the gallery");
    // The first and the last tile that the theme gives a fill other than the default one.
    let differ: Vec<usize> = (0..full.len()).filter(|&i| full.get(i) != none.get(i)).collect();
    let (first, last) = (differ[0], differ[differ.len() - 1]);
    assert!(first < last, "the tiles the theme colours are not one: {differ:?}");
    assert_eq!(some.get(first), full.get(first), "the first tile was worked out");
    assert_eq!(some.get(last), none.get(last), "the last was left to its default when the steps were spent");
}
