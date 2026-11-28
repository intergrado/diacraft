//! The text cells a root style states: what this program assumes when no cell on a shape's way says.
//!
//! **Why the root says it.** The renderer reads a paragraph with no `HorzAlign` anywhere on its way as centred, a line with no `SpLine` as 1.2 lines, and so on (`diacraft_text`'s
//! `ParaFormat`, `BlockFormat` and `CharFormat`). A reader of a file that finds nothing takes the default of the cell's kind, 0, which is left for `HorzAlign`,
//! no line spacing for `SpLine` and a size of 0 for `Size`: the root style is what stands between the two (the root of every file of the census
//! holds the whole `Paragraph` row 0 and all 17 cells of the `Character` row 0, and all nine cells of the text block). The reference application, opening a
//! file written with a root that held no `Paragraph` section, aligned the shapes' text to the left where this program centres it (the first check of written files in it). **A root that states
//! the assumption makes both read one value.**
//!
//! **What is stated.** [`CELLS`]: 12 of the 13 documented cells of the `Paragraph` row 0, the five cells of the text block that the root of a new drawing does not state by itself (the margins
//! are the drawing's own choice, [`crate::Document::new_blank`]), and 12 of the 17 cells of the `Character` row 0, the ones that name no face and no language (a face is the font database's
//! default and a language the document's: this program assumes no value for them, and a root states none). Each value is the one the renderer assumes; `diacraft_render`'s tests hold the two
//! together. **Two cells are left out, `TextPosAfterBullet` and `ColorTrans`**: a file that lacks them reads 0, which is what is assumed, so stating them changes no reading, and an OpenDocument
//! style has no attribute for either, while the rows the engine makes for a formatted run or paragraph hold every cell the row resolves to (the root's row 0 among them) and the `.odg` writer
//! refuses a cell it cannot say.
//!
//! **Two scopes.** [`Scope::Whole`] is the root of a new drawing, which states every cell, as the root of every file of the census does. [`Scope::Reading`] is what the writers of a model that
//! came from no VSDX file, or that hold DiaCraft's own earlier root ([`is_an_earlier_root`]), add to a root that lacks them (a `.diacraft` file made by an earlier build, a drawing read from an
//! `.odg`, a drawing an earlier build made from a template): **only the cells whose absence reads as another value
//! than the assumed one**, which are the cells whose assumed value is not the kind's default 0. A cell the root holds, whatever it holds (a formula, a value of a drawing that was
//! left-aligned on purpose), is never changed; a section the root marks deleted is not brought back.
//!
//! **A text row past the first states the same cells** ([`row_cells`], [`complete_row_cells`]).
//! A style holds row 0 alone, so a `Character` or `Paragraph` row of `IX` 1 or more takes a cell it lacks from the master's row of that `IX` and from nowhere else: a reader of a file
//! that finds none there reads the default of the cell's kind, 0, and the renderer here reads the value it assumes (the same table). The reference application, opening drawings DiaCraft wrote
//! with an org chart title in a `Character` row of `IX` 1 holding `Size` and `Style` alone, drew the title's letters piled on one another, and read the row's `Font` as 0 (the first check of the
//! features, 2026-11-27): a `FontScale` of 0 is letters of no width. Every row of `IX` 1 or more in the files of the census holds all its cells, and a writer that adds a run "writes the
//! whole new row". **A row DiaCraft makes past the first, with no row of its `IX` on a master to merge with, holds every cell of this table in its section**: each one it lacks, with the
//! value the renderer assumes, so that a reader of the file and the renderer read one value. The cells the table leaves out are those whose absence reads as what the renderer assumes in both
//! readers (`ColorTrans` 0, `TextPosAfterBullet` 0) or that name a face or a language, for which this program assumes no value (the font database's default face, the document's language: a
//! `Font` of 0 reads the same).
//!
//! **Whose rows they are, and the value a row of an `.odg` is given**. A writer states them only in a drawing whose rows are DiaCraft's to state ([`rows_are_ours_to_state`]): one that
//! came from no VSDX package, or whose root is DiaCraft's earlier one; the rows of a VSDX file of another program are that program's, and every route writes them as they were read.
//! A cell of a row of an `.odg` is given the value the format reads ([`odf_row_value`]): a paragraph that states no alignment is left-aligned, 0, where the renderer centres.

use std::sync::Arc;

use crate::cell::Cell;
use crate::document::Document;
use crate::name::CellName;
use crate::package::Format;
use crate::part::Style;
use crate::sheet::{CellAddr, Cells, Lookup, Row, RowKey, SectionKey, Sheet};
use crate::value::Value;

/// One cell of the root that states an assumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RootCell {
    /// The section, with the cell in its row 0; `None` for a cell directly under the style (the text block's).
    pub section: Option<&'static str>,
    /// The cell's `N` in a file.
    pub name: &'static str,
    /// The value the renderer assumes for the cell when nothing gives it, as the file holds it.
    pub value: &'static str,
    /// Whether a file's reader that finds no cell reads it as another value than `value` (its kind's default is 0, and `value` is not): the cells [`Scope::Reading`] adds.
    pub reading: bool,
}

const fn cell(section: Option<&'static str>, name: &'static str, value: &'static str, reading: bool) -> RootCell {
    RootCell { section, name, value, reading }
}

/// The cells, in the order a file lists them: the text block's, the `Character` row 0, the `Paragraph` row 0. The values are `diacraft_text`'s defaults (`CharFormat::default`,
/// `ParaFormat::default`, `BlockFormat::default`), written as a file writes them (`Size` in inches, a share as a fraction).
pub const CELLS: &[RootCell] = &[
    // The text block: the middle, the stops every half inch, no background, left to right.
    cell(None, "VerticalAlign", "1", true),
    cell(None, "DefaultTabStop", "0.5", true),
    cell(None, "TextBkgnd", "0", false),
    cell(None, "TextBkgndTrans", "0", false),
    cell(None, "TextDirection", "0", false),
    // The first run: 12 points, black, upright, at full scale, with nothing over, under or through it.
    cell(Some("Character"), "Size", "0.1666667", true),
    cell(Some("Character"), "Color", "0", false),
    cell(Some("Character"), "Style", "0", false),
    cell(Some("Character"), "Case", "0", false),
    cell(Some("Character"), "Pos", "0", false),
    cell(Some("Character"), "FontScale", "1", true),
    cell(Some("Character"), "Letterspace", "0", false),
    cell(Some("Character"), "DblUnderline", "0", false),
    cell(Some("Character"), "Overline", "0", false),
    cell(Some("Character"), "Strikethru", "0", false),
    cell(Some("Character"), "DoubleStrikethrough", "0", false),
    cell(Some("Character"), "ComplexScriptSize", "-1", true),
    // The first paragraph: centred, 1.2 lines to a line, no indent, no space round it, no bullet, left to right.
    cell(Some("Paragraph"), "Bullet", "0", false),
    cell(Some("Paragraph"), "BulletFont", "0", false),
    cell(Some("Paragraph"), "BulletFontSize", "-1", true),
    cell(Some("Paragraph"), "BulletStr", "", false),
    cell(Some("Paragraph"), "Flags", "0", false),
    cell(Some("Paragraph"), "HorzAlign", "1", true),
    cell(Some("Paragraph"), "IndFirst", "0", false),
    cell(Some("Paragraph"), "IndLeft", "0", false),
    cell(Some("Paragraph"), "IndRight", "0", false),
    cell(Some("Paragraph"), "SpAfter", "0", false),
    cell(Some("Paragraph"), "SpBefore", "0", false),
    cell(Some("Paragraph"), "SpLine", "-1.2", true),
];

/// Which of [`CELLS`] a root is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every cell: the root of a new drawing.
    Whole,
    /// The cells whose absence reads as another value than the assumed one ([`RootCell::reading`]): what a writer adds to the root of a model that came from no VSDX file.
    Reading,
}

impl RootCell {
    /// Where the cell is: row 0 of its section, or directly under the style.
    pub fn addr(&self) -> CellAddr {
        match self.section {
            Some(section) => CellAddr::in_row(section, None, RowKey::Ix(0), self.name),
            None => CellAddr::direct(self.name),
        }
    }

    /// Whether `sheet` leaves the cell out: it holds none, in a section it holds or one it lacks. **A cell in a section or row the sheet deletes is not left out** (the file said it has none),
    /// and neither is a cell that holds a formula and no value, or any value: what is there is the file's.
    fn is_missing_in(&self, sheet: &Sheet) -> bool {
        matches!(sheet.lookup(&self.addr()), Lookup::Missing)
    }
}

/// Whether `style` is the root **DiaCraft itself made before it stated the text cells**: the style `No Style` with no `Paragraph` section and a `Character` row 0 that holds a size and a colour
/// and no other cell. Every root of the census holds the whole `Paragraph` section, so no file of another program's holds this one; it is the root of a
/// drawing a build before this one made (a `.diacraft` file, a VSDX file, a drawing made from a template), whatever package data the drawing holds beside it: **a drawing with this root is
/// DiaCraft's own, and what its root lacks is DiaCraft's to state**, which a writer of a model does ([`Scope::Reading`]). A root that is anything else is the file's.
pub fn is_an_earlier_root(style: &Style) -> bool {
    let is_named = |name: &Option<Arc<str>>| name.as_deref() == Some("No Style");
    if !is_named(&style.name_u) || style.sheet.section(&SectionKey::new("Paragraph", None)).is_some() {
        return false;
    }
    let Some(section) = style.sheet.section(&SectionKey::new("Character", None)) else {
        return false;
    };
    let Some(row) = section.row(&RowKey::Ix(0)) else {
        return false;
    };
    row.cells.len() == 2 && row.cells.contains_key("Size") && row.cells.contains_key("Color")
}

/// The cells of `scope` that `style` lacks.
pub fn missing(style: &Style, scope: Scope) -> Vec<RootCell> {
    CELLS.iter().filter(|c| (scope == Scope::Whole || c.reading) && c.is_missing_in(&style.sheet)).copied().collect()
}

/// `style` with the cells of `scope` it lacks added, each with the value the renderer assumes (in its row 0, which is made with its section when there is none), or `None` when it lacks none
/// (the style is then not copied). A cell the style holds is never changed. A cell the sheet refuses (a section or a row past the limits) is left out.
pub fn complete(style: &Style, scope: Scope) -> Option<Style> {
    let lacking = missing(style, scope);
    if lacking.is_empty() {
        return None;
    }
    let mut sheet = Sheet::clone(&style.sheet);
    let mut added = 0usize;
    for cell in lacking {
        // A refusal is a limit of the sheet or a name it will not hold: the cell is then left to the reader's default, as it was.
        if sheet.set_cell(&cell.addr(), Cell::constant(Value::from_text(cell.value))).is_ok() {
            added += 1;
        }
    }
    (added > 0).then(|| Style { sheet: Arc::new(sheet), ..style.clone() })
}

/// The cells of [`CELLS`] in a row of `section` (`Character` or `Paragraph`; none for any other section): what a row of `IX` 1 or more that DiaCraft makes holds whole (
/// see the module's documentation).
pub fn row_cells(section: &str) -> impl Iterator<Item = &'static RootCell> + '_ {
    CELLS.iter().filter(move |c| c.section == Some(section))
}

/// Puts in `cells`, the cells of a row of `section` of `IX` 1 or more, **each cell of [`row_cells`] it lacks**, with the value the renderer assumes. A cell it holds, whatever
/// it holds (a formula, the theme's marker, a value), is never changed. → how many were put in: 0 for a row that holds them all, and for a section that is not a text one.
pub fn complete_row_cells(section: &str, cells: &mut Cells) -> usize {
    let mut added = 0usize;
    for cell in row_cells(section) {
        if !cells.contains_key(cell.name) {
            cells.insert(CellName::new(cell.name), Cell::constant(Value::from_text(cell.value)));
            added = added.saturating_add(1);
        }
    }
    added
}

/// [`complete_row_cells`] for a row of the model, **for the cells `wanted` says yes to** (a writer leaves out a cell the row takes from a master's row of its `IX`), through [`Row::put_cell`], so
/// that the nodes kept among its cells stay where they stood (a cell goes in ahead of the nodes that followed the cell before it). A cell the row holds is never changed, and a cell the row cannot
/// take (a row at the most cells it may hold) is left out, as it is now. → how many were put in.
pub fn complete_row_with(section: &str, row: &mut Row, mut wanted: impl FnMut(&RootCell) -> bool) -> usize {
    complete_row_valued(section, row, |cell| wanted(cell).then_some(cell.value))
}

/// [`complete_row_with`] with **the value each cell is given chosen by the caller**: `value` says `None` for a cell that is left out, and the text a cell holds otherwise (the table's own, or what a
/// row of another origin reads as, [`odf_row_value`]). A cell the row holds is never changed. → how many were put in.
pub fn complete_row_valued(section: &str, row: &mut Row, mut value: impl FnMut(&RootCell) -> Option<&'static str>) -> usize {
    let mut added = 0usize;
    for cell in row_cells(section) {
        if row.cells.contains_key(cell.name) {
            continue;
        }
        let Some(text) = value(cell) else { continue };
        if row.put_cell(&CellName::new(cell.name), Cell::constant(Value::from_text(text))).is_ok() {
            added = added.saturating_add(1);
        }
    }
    added
}

/// The value a cell of [`row_cells`] is given in **a row of the text of an `.odg`**: the table's own, but for the one cell whose absence the format reads otherwise than this program
/// draws it. A paragraph of an `.odg` that states no alignment (no `fo:text-align` on its style or on any style its chain reaches) is left-aligned, which is the format's default and what a program
/// that reads a row lacking `HorzAlign` takes it to be, 0; the renderer here centres a paragraph whose cell nothing gives, and the file's reading is the one the row is completed with, so
/// that the `.odg` the drawing is written back to says nothing of an alignment it never stated, and a paragraph the format left-aligns is not centred by a save across.
pub fn odf_row_value(cell: &RootCell) -> &'static str {
    match (cell.section, cell.name) {
        (Some("Paragraph"), "HorzAlign") => "0",
        _ => cell.value,
    }
}

/// Whether `doc` came from a VSDX package, or holds a part of one: the model writer's own definition of a drawing that has a VSDX file behind it, apart from the one that is only a drawing with
/// unknown elements of its own. A drawing whose window settings were set and that holds nothing else of a package ([`crate::VsdxHeld::holds_windows_alone`]) came from no file.
pub fn came_from_a_vsdx_package(doc: &Document) -> bool {
    doc.vsdx.as_deref().is_some_and(|held| !held.holds_windows_alone()) || doc.parts.iter().any(|part| part.format == Format::Vsdx)
}

/// Whether the text rows past the first of `doc` are **DiaCraft's to state**: a drawing that came from no VSDX package (a new one, one read from an `.odg`, one
/// read from a `.diacraft` file that holds no VSDX package) holds the rows this program and the `.odg` reader made, and a drawing whose root is DiaCraft's own earlier one
/// ([`is_an_earlier_root`]) holds the rows an earlier build made. **A drawing that came from a VSDX package with any other root holds the file's own rows**, which a writer says as they were read
/// on every route, as a save into the file does: whatever a row of the file lacks, the program that wrote it left out, and it is no cell to make up for that program. The same test the model
/// writer applies to the root (`diacraft_vsdx`'s `write::root`).
pub fn rows_are_ours_to_state(doc: &Document) -> bool {
    if !came_from_a_vsdx_package(doc) {
        return true;
    }
    doc.styles.iter().find(|style| style.id == 0).is_some_and(|root| is_an_earlier_root(root))
}

/// Whether `row`, a row of `section`, lacks a cell of [`row_cells`].
pub fn row_lacks(section: &str, row: &Row) -> bool {
    row_cells(section).any(|c| !row.cells.contains_key(c.name))
}
