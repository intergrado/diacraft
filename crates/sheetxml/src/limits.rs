//! The caps of reading sheets, and the error of passing one.
//!
//! The document model has caps of its own for one sheet (`diacraft_doc::MAX_CELLS_PER_CONTAINER`, `MAX_SECTIONS_PER_SHEET`,
//! `MAX_ROWS_PER_SECTION`, `MAX_NAME_BYTES`), and a reader built on this crate can only lower the budgets below, never raise a model cap.
//! The budgets are the caller's, and they are summed over the **whole document** and not over one sheet: a file of ten thousand shapes that
//! each hold the most cells a sheet may is far past what any one sheet's cap allows. Every cap is checked **before** what it counts is
//! stored, and a file past one is refused whole (`LimitError`); nothing is truncated.

/// A cap this codec applies. Each has a class word and the number that was passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// Cells in the whole document.
    Cells,
    /// Cells directly under one sheet, section or row.
    CellsInOnePlace,
    /// Sections in one sheet.
    SectionsInASheet,
    /// Rows in one section.
    RowsInASection,
    /// A cell, section or row name past `MAX_NAME_BYTES`.
    NameBytes,
    /// XML items (elements, attributes and text nodes) modelled or kept, over the whole document.
    Items,
    /// Bytes of memory the document would hold, charged by the real size of what is built (`SheetReader::with_bytes`).
    Bytes,
}

impl LimitKind {
    /// The class word.
    #[must_use]
    pub fn class(self) -> &'static str {
        match self {
            LimitKind::Cells => "too_many_cells",
            LimitKind::CellsInOnePlace => "too_many_cells_in_one_place",
            LimitKind::SectionsInASheet => "too_many_sections_in_a_sheet",
            LimitKind::RowsInASection => "too_many_rows_in_a_section",
            LimitKind::NameBytes => "name_too_long",
            LimitKind::Items => "too_many_items",
            LimitKind::Bytes => "too_much_memory",
        }
    }

    /// What the cap counts, in words: `cells`, `cells in one sheet, section or row`.
    #[must_use]
    pub fn what(self) -> &'static str {
        match self {
            LimitKind::Cells => "cells",
            LimitKind::CellsInOnePlace => "cells in one sheet, section or row",
            LimitKind::SectionsInASheet => "sections in one sheet",
            LimitKind::RowsInASection => "rows in one section",
            LimitKind::NameBytes => "bytes in a cell, section or row name",
            LimitKind::Items => "XML items kept in the document",
            LimitKind::Bytes => "bytes of memory to read the file",
        }
    }
}

/// A cap was passed: which, and its number. The reading is over and what it built is not to be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitError {
    /// Which cap was passed.
    pub kind: LimitKind,
    /// The cap's number: what the file holds more than.
    pub max: u64,
}

impl std::fmt::Display for LimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the file holds more than {} {}", self.max, self.kind.what())
    }
}

impl std::error::Error for LimitError {}

/// The budgets of a document, summed over every sheet read: the caller's choice, and each is checked before what it pays for is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Cells, summed over every sheet of the document.
    pub cells: usize,
    /// XML items kept, summed over every part of the document: each element the reader models or keeps (a cell, a row, a shape, an unknown
    /// element and everything in it), each attribute on one, and each text, comment or instruction it keeps.
    pub items: usize,
    /// Formula nodes the document may parse, summed. A formula the budget cannot afford is kept as text with its cached value, and counted.
    pub formula_nodes: usize,
}
