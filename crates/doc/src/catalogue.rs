//! What the batch specs say about each cell, as data: the class of that decides which sheets a cell
//! is looked for on, the kind of value that gives its default, and the formula a few cells read when they are missing.
//!
//! The table in [`table`] is derived from the cell tables of and the "At a glance" table of the
//! cell batches written so far: transform, geometry, fill and gradient, line and theme, effects and images, text block, text
//! character, page, page layout, actions and layers, and data and links. A cell no batch lists yet (the shape layout, protection and
//! review batches are still to come) is [`CellClass::Unlisted`], and resolves like a cell of the class `style-any` (local, master,
//! root style, default): a root style that holds such a cell is found, and one that does not costs a lookup. The cells of the data and
//! links batch (`Property`, `User`, `Hyperlink`, `MasterSelectors` and `ContentLayout`) are `style-any` by project rule:
//! that is how they resolved while unlisted, and no spec says which sheets MS-VSDX allows these sections on, so listing them changes
//! their kinds and nothing of where they are looked for. Nothing here is
//! a closed list of cells: a name this table does not know is an unknown cell, kept and resolved all the same.
//!
//! A cell the catalogue lists has the kind its batch's Type gives, whatever else the batch says of how it reads: the eight spacing
//! cells of the page layout are `vScalar` there and so [`ValueKind::Scalar`] here, and the defaults the
//! layer batch sets by project rule for a missing `Visible`, `Print`, `Snap`, `Glue` and `Color` are not kinds, so they
//! are the reader's to apply, as `render` does.
//!
//! Names match exactly, in case: `pinx` is not `PinX`.

mod table;

use crate::ValueKind;

/// The three style links, and the family of cells each reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// Line cells, leaving out those in a `FillGradient` section.
    Line,
    /// Fill and effect cells, with those in a `FillGradient` section.
    Fill,
    /// Text cells.
    Text,
}

/// The class of a cell: where its value is looked for, in order, before the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellClass {
    /// `shape-only`: local, master.
    ShapeOnly,
    /// `style-any`: local, master, root style.
    StyleAny,
    /// `fill-family`: local, master, the `FillStyle` chain.
    FillFamily,
    /// `line-family`: local, master, the `LineStyle` chain.
    LineFamily,
    /// `text-family`: local, master, the `TextStyle` chain.
    TextFamily,
    /// `page-sheet`: local, root style.
    PageSheet,
    /// `EnableLineProps`, `EnableFillProps` and `EnableTextProps`: on a style only, read along that style's own link of the
    /// family (our reasoning).
    StyleFlag(Family),
    /// `HideForApply`: on a style only, and the one cell a based-on style does not take: local only.
    StyleLocal,
    /// No batch lists the cell: resolved as `style-any`.
    Unlisted,
}

impl CellClass {
    /// The style family a cell of this class takes from a style chain, if it takes one.
    pub fn family(self) -> Option<Family> {
        match self {
            CellClass::FillFamily => Some(Family::Fill),
            CellClass::LineFamily => Some(Family::Line),
            CellClass::TextFamily => Some(Family::Text),
            _ => None,
        }
    }
}

/// What the catalogue knows of one cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellInfo {
    pub class: CellClass,
    /// The kind of value, which gives the default of a cell found nowhere.
    pub kind: ValueKind,
    /// The formula a missing cell reads instead of that default: the seven `Text Transform` cells
    /// and the two picture size cells.
    /// `calc` evaluates it on the sheet that asked.
    pub default_formula: Option<&'static str>,
    /// Whether the catalogue lists the cell at all.
    pub listed: bool,
}

impl CellInfo {
    const UNLISTED: CellInfo = CellInfo { class: CellClass::Unlisted, kind: ValueKind::Scalar, default_formula: None, listed: false };
}

pub(crate) struct DirectEntry {
    pub name: &'static str,
    pub class: CellClass,
    pub kind: ValueKind,
    pub default_formula: Option<&'static str>,
}

pub(crate) struct RowEntry {
    pub section: &'static str,
    pub name: &'static str,
    pub class: CellClass,
    pub kind: ValueKind,
}

/// What the catalogue says of the cell `name`, directly under a sheet when `section` is `None` and inside a row of the
/// section element `section` otherwise. A cell it does not list is [`CellClass::Unlisted`].
pub fn cell_info(section: Option<&str>, name: &str) -> CellInfo {
    match section {
        None => table::DIRECT
            .binary_search_by(|e| e.name.cmp(name))
            .ok()
            .and_then(|i| table::DIRECT.get(i))
            .map_or(CellInfo::UNLISTED, |e| CellInfo { class: e.class, kind: e.kind, default_formula: e.default_formula, listed: true }),
        Some(section) => row_entry(section, name)
            .or_else(|| (section == "Tabs").then(|| row_entry(section, name.trim_end_matches(|c: char| c.is_ascii_digit()))).flatten())
            .map_or(CellInfo::UNLISTED, |e| CellInfo { class: e.class, kind: e.kind, default_formula: None, listed: true }),
    }
}

fn row_entry(section: &str, name: &str) -> Option<&'static RowEntry> {
    table::ROWS.binary_search_by(|e| (e.section, e.name).cmp(&(section, name))).ok().and_then(|i| table::ROWS.get(i))
}

/// The class of the cells of a section, when every cell the catalogue lists in it has the same one; the class that decides
/// which sheets a whole row is merged from. [`CellClass::Unlisted`] for a section no batch lists.
pub fn section_class(section: &str) -> CellClass {
    let mut classes = table::ROWS.iter().filter(|e| e.section == section).map(|e| e.class);
    match classes.next() {
        Some(first) if classes.all(|c| c == first) => first,
        _ => CellClass::Unlisted,
    }
}

/// The catalogue's own copy of a cell name, so that the many cells with a known name share one allocation.
pub(crate) fn known_cell_name(name: &str) -> Option<&'static str> {
    table::NAMES.binary_search(&name).ok().and_then(|i| table::NAMES.get(i)).copied()
}

/// The names of the 17 predefined sections, which are `Section` elements in a file.
pub const SECTION_NAMES: [&str; 17] = [
    "Geometry",
    "Connection",
    "Control",
    "Scratch",
    "FillGradient",
    "LineGradient",
    "Character",
    "Paragraph",
    "Tabs",
    "Field",
    "Property",
    "User",
    "Hyperlink",
    "Actions",
    "ActionTag",
    "Layer",
    "Reviewer",
];

/// How the rows of a section are keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keying {
    /// By `N`.
    Named,
    /// By `IX`.
    Indexed,
    /// Either: `Connection`, whose rows may be named or not, and any section no spec lists.
    Either,
}

/// How the rows of the section `name` are keyed.
pub fn keying(name: &str) -> Keying {
    match name {
        "User" | "Property" | "Actions" | "Control" | "Hyperlink" | "ActionTag" => Keying::Named,
        "Character" | "Field" | "FillGradient" | "Geometry" | "Layer" | "LineGradient" | "Paragraph" | "Reviewer" | "Scratch" | "Tabs" => {
            Keying::Indexed
        }
        _ => Keying::Either,
    }
}

/// The names of the cells the catalogue lists directly under a sheet, and of those in rows, as `(section, name)`; for tests
/// and tools that check the table against the specs.
pub fn listed_cells() -> impl Iterator<Item = (Option<&'static str>, &'static str, CellClass)> {
    let direct = table::DIRECT.iter().map(|e| (None, e.name, e.class));
    let rows = table::ROWS.iter().map(|e| (Some(e.section), e.name, e.class));
    direct.chain(rows)
}
