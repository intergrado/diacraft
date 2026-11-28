//! The text rows a VSDX file is given.
//!
//! A style holds row 0 alone, so a reader of a VSDX file takes a cell that a `Character` or `Paragraph` row of `IX` 1 or more lacks from the master's row of that `IX` and from nowhere else, and
//! reads it as 0 where there is none: a `FontScale` of 0 piles the letters of the run on one another, a `HorzAlign` of 0 puts the paragraph at the left, a `SpLine` of 0 closes its lines up. This
//! program draws such a cell with the value its renderer assumes (`diacraft_doc::root_text::CELLS`). The rows the program makes are whole (`text.format`'s rows past the first, the rows of the
//! built-in masters); the rows a drawing may hold that are not:
//!
//! * **The rows an `.odg` gives** (`diacraft_odg`'s reader): a run's row holds the cells its style says (the face, the size, the weight), and a paragraph's its alignment.
//! * **The rows of an older build**: a master of the Organization Chart Shapes, of C4, of the network labels, and a run that build's `text.format` made in a drawing whose root stated few cells.
//!
//! **Whose rows they are**: only a drawing whose rows are DiaCraft's to state is given any (`diacraft_doc::root_text::rows_are_ours_to_state`): one that came from no VSDX package, or
//! whose root is the one an earlier build made, as the model writer says of the root. **A drawing that came from a VSDX package with any other root holds the file's own rows**, which every route
//! writes as they were read, as a save into the file does: a program that wrote a row with a size and a style alone left the rest to the master's row of its `IX`, and the rest is no cell to make up
//! for it (an unedited visit through `.diacraft` gave 12 files of the corpus 1,268 cells).
//!
//! **What a row is given** is each cell of `diacraft_doc::root_text::row_cells` that it lacks and that it would not take from elsewhere (a master's row of its `IX`, the row of a style), with the
//! value the drawing itself reads for it: the renderer's (the table's), and for the text of an `.odg` the format's, **a paragraph that states no alignment is left-aligned**
//! (`diacraft_doc::root_text::odf_row_value`). A cell the row holds is never changed, row 0 (which leans on the styles) is left as it is, and a deleted row is left.
//! It is the copy of the drawing a writer is given that is changed: the drawing the person has is not (a save that adopts the model the file reads as, [`super::adopts_by_working_file`], gives the
//! drawing the rows).
//!
//! **The held styles of an `.odg` stand beside the rows given cells** ([`restamp`]): the style each paragraph and run named is stamped by the row it was read with, and a row with cells added is
//! another row to the stamp. The copy's styles are stamped again by the rows as completed, and a group the cells were added to that the style never stated holds a value of no attribute, so the `.odg`
//! the VSDX file is written back to holds the styles, faces and sizes its source held, and no statement of the cells given.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::root_text::{complete_row_valued, odf_row_value, row_lacks, rows_are_ours_to_state};
use diacraft_doc::{
    CellAddr, Document, HeldStyleValue, MAX_SHAPE_DEPTH, OdfShape, OdfTextStyle, Owner, Passthrough, RawAttrs, Resolver, Row, RowKey, Shape,
    ShapeList, ShapeRef, SheetRef, Stamp, Text, TextGroup, Value, stamp_text_group, stamp_text_row,
};

use super::{MAX_STEPS, TOO_BIG};

/// The sections whose rows past the first are made whole.
const TEXT_SECTIONS: [&str; 2] = ["Character", "Paragraph"];

/// One more step of the walk, refused past [`MAX_STEPS`].
fn step(steps: &mut u64, by: u64) -> Result<(), &'static str> {
    *steps = steps.saturating_add(by);
    if *steps > MAX_STEPS { Err(TOO_BIG) } else { Ok(()) }
}

/// Whether the text of `shape` is one an `.odg` gave: the shape holds the data of an element of one, or its text holds the styles its paragraphs and runs named there. The rows of such a text say what the
/// format says: a paragraph that states no alignment is left-aligned ([`odf_row_value`]).
fn is_odg_text(shape: &Shape) -> bool {
    shape.ext.get().is_some_and(|data| data.odf != OdfShape::default())
        || shape.text.as_ref().is_some_and(|text| text.ext.get().is_some_and(|data| !data.odf.paragraphs.is_empty()))
}

/// A row of a shape's text that was given cells, as it was read and as it is now.
struct Completed {
    /// Whether it is a `Paragraph` row (and not a `Character` one).
    paragraph: bool,
    old: Arc<Row>,
    new: Row,
    /// The stamp of `new` as a whole row (made once: a style that stands on the row is stamped by it).
    stamp: Stamp,
}

/// The rows of the `Paragraph` and `Character` sections of `shape`, each with whether it is a paragraph's.
fn text_rows_of(shape: &Shape) -> Vec<(bool, Arc<Row>)> {
    let mut rows = Vec::new();
    for section in shape.sheet.sections.iter().filter(|section| section.ix.is_none() && !section.del) {
        let paragraph = match section.name.as_str() {
            "Paragraph" => true,
            "Character" => false,
            _ => continue,
        };
        rows.extend(section.rows.iter().map(|row| (paragraph, Arc::clone(row))));
    }
    rows
}

/// Whether `row`, as it was read, states anything in `group` through the cells that write its attributes: the alignment through `HorzAlign` alone (`Flags` is its own group, the writing mode), the
/// underline through `DblUnderline` or the underline bit of `Style` (a `Style` with the bit clear writes none), and every other group through any of its cells. A cell that is not a plain number
/// is taken to state.
fn stated(row: &Row, group: TextGroup) -> bool {
    let has = |name: &str| row.cells.contains_key(name);
    match group {
        TextGroup::HorzAlign => has("HorzAlign"),
        TextGroup::Underline => {
            has("DblUnderline")
                || row.cells.get("Style").is_some_and(|cell| match cell.value.as_ref().and_then(Value::number) {
                    Some(bits) => !(0.0..4_294_967_296.0).contains(&bits) || (bits as u32) & 4 != 0,
                    None => true,
                })
        }
        other => other.cells().iter().any(|name| has(name)),
    }
}

/// What stands on a row of the text of an `.odg`: **the held styles beside the paragraphs and runs the file named** (`diacraft_doc::OdfTextStyle`) are stamped by the cells of the row they were
/// read with, and a writer keeps one only while the row it finds is that row. A row that is given cells is another row to the stamp, so the styles beside it are stamped again by
/// the row it now is: **the cells given are the ones the format already said by saying nothing**, and the held style is still what the file stated. Each group the cells given belong to, and for
/// which the style held no value, holds a value of no attribute, which is the statement "the file stated none of this group", so that a writer does not write a cell given as a
/// statement the file never made (a line height of 120% on a paragraph that had none).
///
/// A style is told to stand on a row by the stamp of the row it was read with, which no other row of the shape shares (a row with no cell, which stamps as every row that is not there, and two rows
/// of the same cells are none a style can be told to stand on, and are left as they are).
fn restamp(text: &mut Arc<Text>, done: &[Completed], rows: &[(bool, Arc<Row>)], steps: &mut u64) -> Result<(), &'static str> {
    if text.ext.get().is_none_or(|data| data.odf.paragraphs.is_empty()) {
        return Ok(());
    }
    // The place of each completed row in `done`, by the row as it was read (one lookup for each row below, and not a scan of `done`).
    let place_of: HashMap<(bool, *const Row), usize> = done.iter().enumerate().map(|(at, d)| ((d.paragraph, Arc::as_ptr(&d.old)), at)).collect();
    // Which completed row each stamp stands for, as it was read: `None` where it stands for more than one row.
    let mut stands_for: HashMap<(bool, Stamp), Option<usize>> = HashMap::new();
    for (paragraph, row) in rows {
        step(steps, 1)?;
        let completed = place_of.get(&(*paragraph, Arc::as_ptr(row))).copied().filter(|_| !row.cells.is_empty());
        let key = (*paragraph, stamp_text_row(row));
        let value = if stands_for.contains_key(&key) { None } else { completed };
        stands_for.insert(key, value);
    }
    let completed_for = |paragraph: bool, stamp: Option<Stamp>| -> Option<&Completed> {
        let at = stands_for.get(&(paragraph, stamp?)).copied().flatten()?;
        done.get(at)
    };
    let put = |style: &mut OdfTextStyle, paragraph: bool, steps: &mut u64| -> Result<(), &'static str> {
        step(steps, 1u64.saturating_add(style.held.len() as u64))?;
        if let Some(row) = completed_for(paragraph, style.stamp) {
            step(steps, TextGroup::ALL.len() as u64)?;
            for value in style.held.iter_mut().filter(|v| v.group.is_paragraph() == row.paragraph) {
                if value.stamp == Some(stamp_text_group(&row.old, value.group)) {
                    value.stamp = Some(stamp_text_group(&row.new, value.group));
                }
            }
            for group in TextGroup::ALL.iter().copied().filter(|g| g.is_paragraph() == row.paragraph) {
                let now = stamp_text_group(&row.new, group);
                // A group the row stated before (an alignment that is `justify`, which no held value spells) is written from its cells as it was.
                if stamp_text_group(&row.old, group) != now && !stated(&row.old, group) && !style.held.iter().any(|v| v.group == group) {
                    style.held.push(HeldStyleValue { group, stamp: Some(now), attrs: RawAttrs::default(), kept: Passthrough::new() });
                }
            }
            style.stamp = Some(row.stamp);
        }
        // The values a paragraph holds for its own text are stamped by the whole `Character` row of that text.
        for value in &mut style.held {
            if let Some(row) = completed_for(false, value.stamp) {
                value.stamp = Some(row.stamp);
            }
        }
        Ok(())
    };
    for paragraph in &mut Arc::make_mut(text).ext.make_mut().odf.paragraphs {
        put(&mut paragraph.style, true, steps)?;
        for run in &mut paragraph.runs {
            put(&mut run.style, false, steps)?;
        }
    }
    Ok(())
}

/// The shape at `at` with the text rows past the first given the cells they lack and take from nowhere else. → the copy, when a row changed.
fn fitted(res: &Resolver<'_>, shape: &Shape, at: &ShapeRef, steps: &mut u64) -> Result<Option<Shape>, &'static str> {
    let sheet = SheetRef::Shape(at.clone());
    let odg = is_odg_text(shape);
    let mut own: Option<Shape> = None;
    let mut done: Vec<Completed> = Vec::new();
    for (place, section) in shape.sheet.sections.iter().enumerate() {
        let name = section.name.as_str();
        if !TEXT_SECTIONS.contains(&name) || section.ix.is_some() || section.del {
            continue;
        }
        for (row_at, row) in section.rows.iter().enumerate() {
            step(steps, 1)?;
            let Some(RowKey::Ix(ix)) = row.key(name) else { continue };
            if ix == 0 || row.del || !row_lacks(name, row) {
                continue;
            }
            // A cell the row lacks that a master's row of its `IX` (or a style) gives it is left to them: a reader merges the two rows cell by cell. What is put in is what no step gives.
            let mut made = Row::clone(row);
            let mut walked: Result<(), &'static str> = Ok(());
            let added = complete_row_valued(name, &mut made, |cell| {
                walked = walked.and_then(|()| step(steps, 1));
                let from_elsewhere = walked.is_err() || res.inherits(&sheet, &CellAddr::in_row(name, None, RowKey::Ix(ix), cell.name));
                (!from_elsewhere).then(|| if odg { odf_row_value(cell) } else { cell.value })
            });
            walked?;
            if added == 0 {
                continue;
            }
            let copy = own.get_or_insert_with(|| Shape::clone(shape));
            let sheet_mut = Arc::make_mut(&mut copy.sheet);
            let Some(section_mut) = sheet_mut.sections.get_mut(place) else { continue };
            let section_mut = Arc::make_mut(section_mut);
            if let Some(slot) = section_mut.rows.get_mut(row_at) {
                done.push(Completed { paragraph: name == "Paragraph", old: Arc::clone(row), stamp: stamp_text_row(&made), new: made.clone() });
                *slot = Arc::new(made);
            }
        }
    }
    // The held styles of an `.odg`'s paragraphs and runs stand beside the rows that were given cells (`restamp`).
    if odg
        && !done.is_empty()
        && let Some(copy) = own.as_mut()
        && let Some(text) = copy.text.as_mut()
    {
        restamp(text, &done, &text_rows_of(shape), steps)?;
    }
    Ok(own)
}

/// The shapes of `list` (the shapes of `owner` at `path`) with their text rows given what they lack, group members included. → the list, when a shape changed.
fn fitted_list(
    res: &Resolver<'_>,
    list: &ShapeList,
    owner: Owner,
    path: &[u32],
    steps: &mut u64,
    count: &mut u64,
) -> Result<Option<ShapeList>, &'static str> {
    if path.len() >= MAX_SHAPE_DEPTH {
        return Err(TOO_BIG);
    }
    let mut kept: Vec<Arc<Shape>> = Vec::new();
    let mut changed = false;
    for (place, shape) in list.iter().enumerate() {
        step(steps, 1)?;
        let mut here = path.to_vec();
        here.push(u32::try_from(place).map_err(|_| TOO_BIG)?);
        let at = ShapeRef { owner, path: here };
        let mut copy = fitted(res, shape, &at, steps)?;
        *count = count.saturating_add(u64::from(copy.is_some()));
        if !shape.children.is_empty()
            && let Some(members) = fitted_list(res, &shape.children, owner, &at.path, steps, count)?
        {
            copy.get_or_insert_with(|| Shape::clone(shape)).children = members;
        }
        match copy {
            Some(own) => {
                changed = true;
                kept.push(Arc::new(own));
            }
            None => kept.push(Arc::clone(shape)),
        }
    }
    Ok(changed.then(|| ShapeList::from_vec(kept)))
}

/// The drawing with every text row past the first of a page's or a master's shapes given the cells it lacks (see the module documentation), and how many shapes were; `None` when none is (and
/// then the count is 0). The copy shares everything it does not change.
pub(crate) fn fit(doc: &Document, steps: &mut u64) -> Result<(u64, Option<Document>), &'static str> {
    // **A drawing that came from a VSDX package with the root of another program holds that program's rows, which are written as they were read**: the rows an `.odg` gave and
    // the rows an earlier build made are the ones that lack cells for a reason this program can state.
    if !rows_are_ours_to_state(doc) {
        return Ok((0, None));
    }
    let res = Resolver::new(doc);
    let mut count = 0u64;
    let mut out: Option<Document> = None;
    for at in 0..doc.pages.len() {
        let Some(page) = doc.pages.get(at) else { continue };
        if let Some(shapes) = fitted_list(&res, &page.shapes, Owner::Page(page.id), &[], steps, &mut count)? {
            let mut copy = diacraft_doc::Page::clone(page);
            copy.shapes = shapes;
            // (`at` is a place in the list.)
            let _ = out.get_or_insert_with(|| doc.clone()).pages.set(at, Arc::new(copy));
        }
    }
    for at in 0..doc.masters.len() {
        let Some(master) = doc.masters.get(at) else { continue };
        if let Some(shapes) = fitted_list(&res, &master.shapes, Owner::Master(master.id), &[], steps, &mut count)? {
            let mut copy = diacraft_doc::Master::clone(master);
            copy.shapes = shapes;
            let _ = out.get_or_insert_with(|| doc.clone()).masters.set(at, Arc::new(copy));
        }
    }
    Ok((count, out))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{Cell, CellName, OdfParagraph, OdfRun};

    use super::*;

    /// A row of one cell: its name and its value.
    fn row_of(name: &str, value: &str) -> Row {
        let mut row = Row::default();
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
        row
    }

    /// A drawing of one page and one shape, **an `.odg`'s text of `n` paragraphs**: each on a `Paragraph` row of its own (an indent alone) and holding a run on a `Character` row of its own (a size
    /// alone), each paragraph and run with the held style stamped by its row, as the `.odg` reader makes them.
    fn odg_text_of(n: u32) -> Document {
        let mut shape = Shape::new(1);
        let mut text = Text::default();
        let mut paragraphs = Vec::new();
        {
            let sheet = Arc::make_mut(&mut shape.sheet);
            for ix in 1..=n {
                let (size, indent) = (row_of("Size", &format!("0.{ix}")), row_of("IndLeft", &format!("0.{ix}")));
                sheet.set_cell(&CellAddr::in_row("Character", None, RowKey::Ix(ix), "Size"), size.cells.get("Size").unwrap().clone()).unwrap();
                sheet
                    .set_cell(&CellAddr::in_row("Paragraph", None, RowKey::Ix(ix), "IndLeft"), indent.cells.get("IndLeft").unwrap().clone())
                    .unwrap();
                let style = |row: &Row| diacraft_doc::OdfTextStyle { name: None, stamp: Some(stamp_text_row(row)), held: Vec::new() };
                paragraphs.push(OdfParagraph {
                    style: style(&indent),
                    runs: vec![OdfRun { from: 0, to: 1, style: style(&size), element: None }],
                    ..OdfParagraph::default()
                });
            }
        }
        text.ext.make_mut().odf.paragraphs = paragraphs;
        shape.text = Some(Arc::new(text));
        let mut doc = Document::new_blank();
        let mut page = diacraft_doc::Page::clone(doc.pages.get(0).unwrap());
        page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        doc.pages.set(0, Arc::new(page)).unwrap();
        doc
    }

    /// **The work is counted in proportion to the rows** (the security review): the stamps of the rows are looked up by hash and not by a scan of the rows given cells, so the steps
    /// charged double with the rows, and every paragraph and run is stamped again by the row it stands on.
    #[test]
    fn the_work_of_standing_the_held_styles_on_their_rows_is_counted_by_the_rows() {
        let mut spent = Vec::new();
        for n in [5_000u32, 10_000] {
            let doc = odg_text_of(n);
            let mut steps = 0u64;
            let (count, with) = fit(&doc, &mut steps).unwrap();
            assert_eq!(count, 1, "the shape is given the cells its rows lack");
            let with = with.unwrap();
            let shape = with.pages.get(0).unwrap().shapes.get(0).unwrap();
            let paragraphs = &shape.text.as_ref().unwrap().ext.get().unwrap().odf.paragraphs;
            // Each paragraph stands on its row as the row is now: the stamp of the row's cells, whole.
            let rows: Vec<&Arc<Row>> = shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Paragraph").flat_map(|s| s.rows.iter()).collect();
            assert_eq!(rows.len(), n as usize);
            for (row, paragraph) in rows.iter().zip(paragraphs) {
                assert_eq!(paragraph.style.stamp, Some(stamp_text_row(row)));
                assert!(!row_lacks("Paragraph", row), "the row is whole");
            }
            assert!(steps >= u64::from(n), "every row is a step: {steps}");
            spent.push(steps);
        }
        assert!(spent[1] <= spent[0] * 2 + 64, "the work doubles with the rows: {spent:?}");
    }

    /// **The budget is the walk's**: a drawing whose walk starts at the end of the budget is refused (`TOO_BIG`), and not walked.
    #[test]
    fn a_text_of_many_rows_is_refused_past_the_budget() {
        let doc = odg_text_of(100);
        let mut steps = MAX_STEPS.saturating_sub(5);
        assert_eq!(fit(&doc, &mut steps).err(), Some(TOO_BIG));
    }

    /// The most rows a section holds (320,000), each one given cells and each stood on by a held style, **in the time of a walk**: a scan of the rows given cells for each row of the shape is 10^11
    /// comparisons here, and the lookup is 320,000. Real time, so it runs by hand (`cargo test -p diacraft-engine --lib -- --ignored the_most_rows`).
    #[test]
    #[ignore = "real time: run by hand"]
    fn the_most_rows_a_section_holds_are_stood_on_in_a_walk() {
        let doc = odg_text_of(300_000);
        let began = std::time::Instant::now();
        let mut steps = 0u64;
        let (count, _) = fit(&doc, &mut steps).unwrap();
        assert_eq!(count, 1);
        assert!(began.elapsed().as_secs() < 60, "{:?} for {steps} steps", began.elapsed());
    }
}
