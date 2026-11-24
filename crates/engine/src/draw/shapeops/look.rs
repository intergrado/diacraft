//! What a produced shape takes from the first shape: its text and its formatting (SHAPEOPS-PROPS, SHAPEOPS-LAYER).
//!
//! **Copied from the first shape**, formula and value as they stand, where the first shape holds them itself or inherits them from its master (a shape that is an
//! instance of a master is the usual first shape, and the produced shape has no master to inherit from):
//!
//! * the three style links, as the first shape has them (its own, or its master shape's, which is the link it reads);
//! * every direct cell of the three families the catalogue lists: Line Format, Fill Format with the shadow, gradient and effect cells (glow, soft edges, bevel,
//!   reflection, 3-D rotation, sketch), the theme and Quick Style cells, and the Text Block Format cells (margins, alignment, text background);
//! * the `FillGradient` and `LineGradient` rows, as the shape reads them;
//! * `LayerMember` (SHAPEOPS-LAYER);
//! * where the command gives text, the text with its runs (the first shape's own, or its master's) and the `Character`, `Paragraph` and `Tabs` rows it
//!   names: a command that gives none (Fragment) gives neither, since a first shape may hold a thousand rows of each and a Fragment may make ten thousand shapes.
//!
//! **The sections are shared.** A command that makes many shapes holds the sections once, as `Arc`s, and each made shape holds the same ones ([`super::apply`]), and
//! what they cost to write ([`section_units`], rows and cells) is charged for each shape made before any is built (`MAX_LOOK_UNITS`).
//!
//! A value a style supplies is not copied: the produced shape reads the same style through the same link.
//!
//! **Not carried**: Shape Data, User-defined cells, Hyperlinks, Actions, Action Tags, Controls, Connection Points, Scratch, Events, Protection,
//! Miscellaneous, Glue Info and Shape Layout. A formula that names anything the produced shape lacks is replaced by its value ([`freeze_unportable`]), and a text
//! field by the text it shows.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellClass, CellName, FormulaSlot, Links, Resolved, Resolver, Row, RowKey, Section, ShapeRef, SheetRef, Source, Text, TextPart,
    Value, listed_cells,
};
use diacraft_formula::Expr;

use crate::draw::{Problem, Reader};

/// The sections whose rows are copied, as the shape reads them: the formatting of the shape's fill and line, and the formatting of its text.
const SECTIONS: [&str; 5] = ["FillGradient", "LineGradient", "Character", "Paragraph", "Tabs"];

/// How many of [`SECTIONS`], from the first, are the shape's and not its text's: a made shape that has no text (Fragment's) has no use for the others, which are the
/// text's runs, paragraphs and tabs, and a first shape may hold a thousand rows of each.
const SECTIONS_OF_THE_SHAPE: usize = 2;

/// The most rows of one such section that are copied (counted over the shape, its master and its styles). A section with more is refused in words, and not left out.
const MAX_ROWS: usize = 1_024;

/// The most sections of one name that are copied (a shape has one of each of these, or a few by `IX`). A shape with more is refused in words, and not cut short.
const MAX_SECTIONS: usize = 8;

/// What is taken from the first shape.
#[derive(Debug, Default)]
pub(super) struct Look {
    pub links: Links,
    pub cells: Vec<(CellName, Cell)>,
    pub sections: Vec<Section>,
    /// The first shape's text with its fields flattened, or `None`.
    pub text: Option<Arc<Text>>,
    /// Its places in containers and lists (SHAPEOPS-CONTAINERS): for a command that makes one shape, which takes them.
    pub ties: Option<Cell>,
}

/// A cell as the produced shape is to hold it, from what the first shape resolves it to: `None` when a style supplies it or it is found nowhere (the produced
/// shape reads the style through the same link), a formula of its own or of its master with the value it has, or a constant.
fn copied(found: &Resolved<'_>) -> Option<Cell> {
    let here = |source: &Source| matches!(source, Source::Local | Source::Master(_));
    let formula_here = here(&found.formula.source);
    let value_here = here(&found.value.source);
    if !formula_here && !value_here {
        return None;
    }
    let value = found.value.cell.and_then(|c| c.value.clone());
    let unit = found.value.cell.and_then(|c| c.unit.clone()).or_else(|| found.formula.cell.and_then(|c| c.unit.clone()));
    // A cell of the shape's own or its master's that inherits its formula from a style (`F="Inh"`, as a theme's colours are held) goes on inheriting it: the made
    // shape has the style's link too, so the formula it reads is the same one. Anything else of a style's is not copied.
    let inherits_a_style =
        matches!(found.formula.source, Source::Style { .. }) && found.value.cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Inherit));
    let mut cell = match found.formula.cell.map(|c| &c.formula) {
        Some(FormulaSlot::Local(formula)) if formula_here => Cell::with_formula(&formula.source, value),
        _ if inherits_a_style => Cell::inherited(value),
        _ => Cell::constant(value?),
    };
    cell.unit = unit;
    Some(cell)
}

/// The three links the shape reads: its own, and for a family it has none of, its master shape's.
fn effective_links(res: &Resolver<'_>, at: &ShapeRef) -> Links {
    let own = res.shape(at).map(|s| s.sheet.links).unwrap_or_default();
    let master = res.master_step(at).ok().flatten().and_then(|step| res.shape(&step.shape)).map(|s| s.sheet.links).unwrap_or_default();
    Links { line: own.line.or(master.line), fill: own.fill.or(master.fill), text: own.text.or(master.text) }
}

/// The text the shape shows: its own, or when it has none its master shape's.
fn text_of(res: &Resolver<'_>, at: &ShapeRef) -> Option<Arc<Text>> {
    let shape = res.shape(at)?;
    if let Some(own) = &shape.text {
        return Some(Arc::clone(own));
    }
    match res.master_step(at) {
        Ok(Some(step)) => res.shape(&step.shape).and_then(|m| m.text.clone()),
        _ => None,
    }
}

/// The text with each field replaced by the text it shows at that moment: the runs and markers stay.
fn flattened(text: &Text) -> Text {
    let parts = text
        .parts
        .iter()
        .map(|part| match part {
            TextPart::Fld { text, .. } => TextPart::Chars(Arc::clone(text)),
            other => other.clone(),
        })
        .collect();
    Text { parts, ..Default::default() }
}

/// The names of the direct cells that are copied: those of the three families the catalogue lists, and `LayerMember`.
fn copied_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = listed_cells()
        .filter(|(section, _, class)| section.is_none() && matches!(class, CellClass::FillFamily | CellClass::LineFamily | CellClass::TextFamily))
        .map(|(_, name, _)| name)
        .collect();
    names.push("LayerMember");
    names
}

/// The rows of a section as the shape at `at` reads them, with the cells that the shape or its master holds. A section that is more than a command copies is refused in
/// words (the first shape's formatting would otherwise lose it whole and say nothing): its sections past [`MAX_SECTIONS`], or its rows past [`MAX_ROWS`].
fn section_copy(res: &Resolver<'_>, sheet: &SheetRef, name: &str) -> Result<Vec<Section>, Problem> {
    let keys = match res.effective_sections_within(sheet, name, MAX_SECTIONS) {
        None => return Ok(Vec::new()),
        Some(Ok(keys)) => keys,
        Some(Err(n)) => {
            return Err(Problem::failed(format!("the first shape has {n} `{name}` sections, and a command copies at most {MAX_SECTIONS} of them")));
        }
    };
    let mut out = Vec::new();
    for key in keys {
        let rows = match res.effective_rows_within(sheet, &key, MAX_ROWS) {
            None => continue,
            Some(Ok(rows)) => rows,
            Some(Err(n)) => {
                return Err(Problem::failed(format!(
                    "the first shape has {n} rows in its `{name}` section, and a command copies at most {MAX_ROWS} of them"
                )));
            }
        };
        let mut section = Section::new(name, key.ix);
        for effective in rows {
            let cells: Vec<(CellName, Cell)> = effective.cells.iter().filter_map(|(n, found)| copied(found).map(|c| (n.clone(), c))).collect();
            if cells.is_empty() {
                continue;
            }
            let mut row = Row { row_type: effective.row_type.clone(), cells: cells.into_iter().collect(), ..Row::default() };
            match &effective.key {
                RowKey::Ix(ix) => row.ix = Some(*ix),
                RowKey::Name(n) => row.name = Some(Arc::clone(n)),
            }
            section.rows.push(Arc::new(row));
        }
        if !section.rows.is_empty() {
            out.push(section);
        }
    }
    Ok(out)
}

/// What a section costs to write into a shape: one for it, one for each cell of its own, and for each row one and one for each of its cells. A made shape holds the
/// section and a file written from it writes all of this, whether or not the shapes share it, so the cost is charged for each shape made.
pub(super) fn section_units(section: &Section) -> usize {
    let rows = section.rows.iter().fold(0_usize, |sum, row| sum.saturating_add(1).saturating_add(row.cells.len()));
    rows.saturating_add(1).saturating_add(section.cells.len())
}

/// What the produced shapes take from the shape at `at`; `with_text` says whether the command gives text. A section that is more than a command copies is refused.
pub(super) fn capture(reader: &Reader<'_>, at: &ShapeRef, with_text: bool) -> Result<Look, Problem> {
    let res = reader.resolver();
    let sheet = SheetRef::Shape(at.clone());
    let mut look = Look { links: effective_links(res, at), ..Look::default() };
    for name in copied_names() {
        if let Some(cell) = res.resolve(&sheet, &CellAddr::direct(name)).and_then(|found| copied(&found)) {
            look.cells.push((CellName::new(name), cell));
        }
    }
    // A command that gives no text (Fragment) gives no `Character`, `Paragraph` or `Tabs` rows either: they are the text's.
    let wanted = if with_text { SECTIONS.len() } else { SECTIONS_OF_THE_SHAPE };
    for name in SECTIONS.iter().take(wanted) {
        look.sections.extend(section_copy(res, &sheet, name)?);
    }
    if with_text {
        look.text = text_of(res, at).map(|t| Arc::new(flattened(&t)));
    }
    look.ties = super::ties::carried(reader, &sheet);
    Ok(look)
}

/// Whether every reference of the formula is to a direct cell of the produced shape: no other shape, no section, no qualifier. The walk is iterative.
fn portable(expr: &Expr, have: &BTreeSet<CellName>) -> bool {
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Ref(r) => {
                let direct = r.qualifiers.is_empty() && r.path.len() == 1;
                let name_ok = r.path.first().is_some_and(|s| s.index.is_none() && have.contains(s.name.as_ref()));
                if !(direct && name_ok) {
                    return false;
                }
            }
            Expr::Call(c) => {
                if !c.qualifiers.is_empty() {
                    return false;
                }
                stack.extend(c.args.iter().flatten());
            }
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
        }
    }
    true
}

/// The cell as a constant of the value it holds, where its formula names something the produced shape lacks. A formula that does not parse is kept as
/// it is: no one can say what it names.
fn frozen(cell: &Cell, have: &BTreeSet<CellName>) -> Option<Cell> {
    let FormulaSlot::Local(formula) = &cell.formula else { return None };
    let Ok(expr) = &formula.ast else { return None };
    if portable(expr, have) {
        return None;
    }
    let mut constant = Cell::constant(cell.value.clone().unwrap_or_else(|| Value::from_text("0")));
    constant.unit = cell.unit.clone();
    Some(constant)
}

/// Replaces, in the cells of `cells` and of the rows of `sections`, each formula that names a cell the produced shape lacks by its value. `have` holds the direct
/// cells the produced shape has.
pub(super) fn freeze_unportable(cells: &mut [(CellName, Cell)], sections: &mut [Section], have: &BTreeSet<CellName>) {
    for (_, cell) in cells.iter_mut() {
        if let Some(constant) = frozen(cell, have) {
            *cell = constant;
        }
    }
    for section in sections.iter_mut() {
        let rows: Vec<Arc<Row>> = section
            .rows
            .iter()
            .map(|row| {
                let mut row = (**row).clone();
                for cell in row.cells.values_mut() {
                    if let Some(constant) = frozen(cell, have) {
                        *cell = constant;
                    }
                }
                Arc::new(row)
            })
            .collect();
        section.rows = rows.into();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn have(names: &[&str]) -> BTreeSet<CellName> {
        names.iter().map(|n| CellName::new(n)).collect()
    }

    fn cell(formula: &str) -> Cell {
        let mut c = Cell::with_formula(formula, Some(Value::from_text("7")));
        c.unit = Some(Arc::from("IN"));
        c
    }

    #[test]
    fn a_formula_of_direct_cells_the_shape_has_is_kept_and_any_other_is_its_value() {
        let names = have(&["Width", "Height", "FillForegnd", "PinX"]);
        assert!(frozen(&cell("Width*0.5"), &names).is_none());
        assert!(frozen(&cell("THEMEVAL(\"FillColor\")"), &names).is_none());
        assert!(frozen(&cell("IF(Width>1,FillForegnd,2)"), &names).is_none());
        for naming_something_missing in
            ["Prop.Cost", "Sheet.4!PinX", "TheText", "Char.Size", "Geometry1.X1", "SHAPETEXT(TheText)", "Sheet.2!SHEETREF()"]
        {
            let constant = frozen(&cell(naming_something_missing), &names).unwrap_or_else(|| panic!("{naming_something_missing} is not portable"));
            assert_eq!(constant.value.as_ref().unwrap().text(), "7");
            assert_eq!(constant.unit.as_deref(), Some("IN"));
            assert!(constant.formula.text().is_none());
        }
    }

    #[test]
    fn a_field_becomes_the_text_it_shows_and_the_markers_stay() {
        let text = Text {
            parts: vec![TextPart::Cp(1), TextPart::Chars(Arc::from("Page ")), TextPart::Fld { ix: 0, text: Arc::from("3") }],
            ..Default::default()
        };
        let flat = flattened(&text);
        assert_eq!(flat.parts.len(), 3);
        assert!(matches!(&flat.parts[0], TextPart::Cp(1)));
        assert!(matches!(&flat.parts[2], TextPart::Chars(c) if &**c == "3"));
    }

    #[test]
    fn the_cells_copied_are_the_family_cells_and_not_the_ones_that_say_where_a_shape_is() {
        let names = copied_names();
        for wanted in ["FillForegnd", "FillPattern", "LinePattern", "LineWeight", "ShapeShdwShow", "GlowSize", "LeftMargin", "LayerMember"] {
            assert!(names.contains(&wanted), "{wanted}");
        }
        for not in ["PinX", "Width", "Angle", "BeginX", "EnableFillProps", "PageWidth"] {
            assert!(!names.contains(&not), "{not}");
        }
    }
}
