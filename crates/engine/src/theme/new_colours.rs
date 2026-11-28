//! The colours a new shape reads through the theme by formula.
//!
//! A built-in master whose fill is a formula of `THEMEVAL` (a C4 element's accent: `THEMEVAL("AccentColor", RGB(58,127,192))`) is built with no theme, so its cached colour is the default.
//! A shape dropped from it into a drawing whose page has a theme inherits that colour, and **follows the theme** only once it is linked to the `Theme` style
//! ([`super::apply::follow_page_theme`]): the theme then answers the formula, and the instance must be given what it answers or it shows the default until something recalculated it. So after the
//! links are made, the cells of the shape's masters that call `THEMEVAL` are worked out for the instance and the members it lists (and nothing else is), and each result that differs from the
//! master's cached colour is written to the instance as the result of an evaluation (`Inh`), as a recalculation writes it.
//!
//! **Bounded.** At most [`MAX_SHAPES`] shapes (the instance and its members) and [`MAX_CELLS`] cells of each master shape are looked at, one evaluator with the default budget does the work, and a
//! budget that runs out refuses the drop in words and leaves the document as it was (the edit is rolled back by the command).

use diacraft_calc::{Budget, CellAddress, EvalCtx, Evaluator, Value as Calc};
use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellWrite, Document, FormulaSlot, MAX_SHAPE_DEPTH, Owner, Resolver, RowKey, Shape, ShapeRef, Sheet, SheetRef, Value,
    WriteOrigin,
};

use crate::draw::Problem;

/// The most shapes (an instance and the members it lists) whose masters' colours are worked out for one new shape.
const MAX_SHAPES: usize = 256;

/// The most cells of one master shape that call `THEMEVAL` and are worked out.
const MAX_CELLS: usize = 64;

/// The cells of a sheet whose formula calls `THEMEVAL`: the sheet's own, and those of the rows of its sections, at most [`MAX_CELLS`].
fn cells_that_read_the_theme(sheet: &Sheet) -> Vec<CellAddr> {
    let reads = |cell: &Cell| matches!(&cell.formula, FormulaSlot::Local(f) if f.source.contains("THEMEVAL"));
    let mut found: Vec<CellAddr> = sheet.cells.iter().filter(|(_, cell)| reads(cell)).map(|(name, _)| CellAddr::direct(name.as_str())).collect();
    for section in &sheet.sections {
        let name = section.name.as_str();
        found.extend(section.cells.iter().filter(|(_, cell)| reads(cell)).map(|(cell, _)| CellAddr::in_section(name, section.ix, cell.as_str())));
        for row in &section.rows {
            let key = match (&row.name, row.ix) {
                (Some(named), _) => RowKey::name(named),
                (None, Some(ix)) => RowKey::Ix(ix),
                (None, None) => continue,
            };
            found.extend(
                row.cells.iter().filter(|(_, cell)| reads(cell)).map(|(cell, _)| CellAddr::in_row(name, section.ix, key.clone(), cell.as_str())),
            );
        }
        if found.len() >= MAX_CELLS {
            break;
        }
    }
    found.truncate(MAX_CELLS);
    found
}

/// The places of a shape and of the shapes inside it, the shape first, at most [`MAX_SHAPES`] and no deeper than the model allows.
fn places(at: &ShapeRef, shape: &Shape) -> Vec<ShapeRef> {
    let mut out = vec![at.clone()];
    let mut work: Vec<(ShapeRef, &Shape, usize)> = vec![(at.clone(), shape, 1)];
    while let Some((place, parent, depth)) = work.pop() {
        if depth > MAX_SHAPE_DEPTH {
            continue;
        }
        for (k, child) in parent.children.iter().enumerate() {
            let Ok(k) = u32::try_from(k) else { break };
            if out.len() >= MAX_SHAPES {
                return out;
            }
            let here = place.child(k);
            out.push(here.clone());
            work.push((here, child, depth + 1));
        }
    }
    out
}

/// Works out for the new shape `id` of `page`, and the members it lists, the colours its masters give by a formula of `THEMEVAL`, now that the shape follows the page's theme.
pub(super) fn settle(doc: &mut Document, page: u32, id: u32) -> Result<(), Problem> {
    let Some(top) = doc.find_shape(Owner::Page(page), id) else { return Ok(()) };
    let Some(shape) = doc.shape(&top) else { return Ok(()) };
    // First the cells that read the theme, by the masters alone: a shape whose masters have none (nearly every shape) costs no evaluator.
    let reading: Vec<(ShapeRef, ShapeRef, Vec<CellAddr>)> = {
        let res = Resolver::new(doc);
        places(&top, shape)
            .into_iter()
            .filter_map(|at| {
                let step = res.master_step(&at).ok().flatten()?;
                let cells = cells_that_read_the_theme(&doc.shape(&step.shape)?.sheet);
                (!cells.is_empty()).then_some((at, step.shape, cells))
            })
            .collect()
    };
    if reading.is_empty() {
        return Ok(());
    }
    let mut writes: Vec<(SheetRef, CellWrite)> = Vec::new();
    let mut ctx = EvalCtx::new(crate::measure::eval_options(), Budget::default());
    {
        let res = Resolver::new(doc);
        let mut ev = Evaluator::new(doc, &mut ctx);
        for (at, source, cells) in reading {
            for addr in cells {
                let Calc::Color(rgb) = ev.evaluate(&CellAddress::new(SheetRef::Shape(at.clone()), addr.clone())) else { continue };
                let text = format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b);
                let held = res
                    .resolve(&SheetRef::Shape(source.clone()), &addr)
                    .and_then(|found| found.value.cell?.value.as_ref().map(|v| v.text().to_owned()));
                if held.is_some_and(|held| held.eq_ignore_ascii_case(&text)) {
                    continue;
                }
                let edit = CellEdit::Cached { value: Value::from_text(&text), error: None, unit: None };
                writes.push((SheetRef::Shape(at.clone()), CellWrite { addr, edit, origin: WriteOrigin::Evaluation }));
            }
        }
    }
    if ctx.stats.budget_exhausted || ctx.stats.memory_exhausted {
        return Err(Problem::failed("the page is too large to work out the new shape's theme colours"));
    }
    if writes.is_empty() {
        return Ok(());
    }
    doc.write_batch(writes).map(|_| ()).map_err(|e| Problem::failed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::ShapeList;

    use super::*;
    use crate::stencil::{ensure_master, find_master};

    /// The cells of the shape of a built-in master that call `THEMEVAL`.
    fn reading(master: &str) -> Vec<String> {
        let mut doc = Document::new_blank();
        let id = ensure_master(&mut doc, find_master(master).unwrap()).unwrap();
        let shape = doc.master(id).unwrap().shapes.get(0).unwrap().clone();
        cells_that_read_the_theme(&shape.sheet).iter().map(|addr| format!("{addr:?}")).collect()
    }

    #[test]
    fn a_master_whose_fill_and_text_are_theme_formulas_names_those_cells_and_a_plain_master_names_none() {
        let person = reading("c4.person");
        assert!(person.iter().any(|a| a.contains("FillForegnd")), "the fill: {person:?}");
        assert!(
            person.iter().any(|a| a.contains("Character") && a.contains("Color")),
            "the text colour of a row of the Character section: {person:?}"
        );
        assert!(reading("rectangle").is_empty(), "a master with no theme formula costs nothing");
        assert!(reading("c4.infrastructure-node").iter().all(|a| !a.contains("FillForegnd")), "the infrastructure node keeps the style's fill");
    }

    #[test]
    fn a_shape_with_very_many_cells_that_read_the_theme_names_at_most_the_cap() {
        let mut sheet = Sheet::default();
        for k in 0..(3 * MAX_CELLS) {
            sheet.cells.insert(diacraft_doc::CellName::new(&format!("User{k}")), Cell::with_formula("THEMEVAL(\"AccentColor\")", None));
        }
        assert_eq!(cells_that_read_the_theme(&sheet).len(), MAX_CELLS);
    }

    #[test]
    fn the_places_of_a_shape_are_it_first_and_its_members_up_to_the_cap() {
        let at = ShapeRef::top(Owner::Page(1), 0);
        let mut shape = Shape::new(1);
        let members: Vec<Arc<Shape>> = (2..2 + 3 * MAX_SHAPES as u32).map(|id| Arc::new(Shape::new(id))).collect();
        shape.children = ShapeList::from_vec(members);
        let found = places(&at, &shape);
        assert_eq!(found.len(), MAX_SHAPES, "the cap holds");
        assert_eq!(found[0], at, "the shape is first");
        assert_eq!(found[1], at.child(0), "then its members in order");
        assert_eq!(places(&at, &Shape::new(1)), vec![at.clone()], "a shape with no member is its own place");
    }
}
