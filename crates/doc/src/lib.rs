//! The DiaCraft document model: the ShapeSheet.
//!
//! **A document is sheets.** Its pages, masters, styles and shapes each hold a [`Sheet`] of cells, and a cell holds a formula and
//! a cached value with a unit (`AGENTS.md`, Architecture). There is no second source of truth: a shape's position, size,
//! geometry, text transform and look are cells, and nothing here caches any of them. A [`Document`] is what `visio/document.xml`
//! and the parts it reaches hold.
//!
//! **Verbatim.** The text of a formula, a value, a unit and an error is held as the file gave it, and what the model has no
//! field for is kept in [`RawAttrs`] and [`Passthrough`]. A typed reading of a value
//! is derived from its text on each call ([`Value`]), so the two cannot disagree.
//!
//! **Inheritance** is a lookup, not a copy. A sheet holds only what is local. A cell it
//! does not hold is found on its master, then on a style, then defaults ([`Resolver::resolve`]), and the answer says where
//! it came from. Nothing here evaluates a formula: [`Resolved`] hands `calc` the cell to evaluate and the sheet to evaluate it
//! on, which is always the inheritor, and the map from `Sheet.N` in a master's formula to the inheritor's own shapes.
//!
//! **Sharing.** Pages, masters, styles, sheets, sections, rows and shapes are behind `Arc`, and a page's shapes are a
//! persistent list ([`SharedVec`]), so a clone of a document (an undo step) copies reference counts, and writing one cell copies
//! the few nodes on the way to it. The cost is in `Document::write_cell`.
//!
//! **Hostile input.** An imported document can hold anything: a style link to nowhere or in a cycle, a `Master` that names no
//! master, shape IDs used twice, groups nested to any depth. Nothing here indexes without `.get()`, nothing recurses without a
//! bound, and every walk of a link is capped ([`MAX_STYLE_STEPS`], [`MAX_SHAPE_DEPTH`]).
//!
//! **Two formats, tagged.** The model holds both the VSDX format's data and ODF's. The existing `raw` and `passthrough` fields (and [`TextPart::Raw`]) stay
//! VSDX's; each object that can carry more has one extension slot, [`package::Ext`] (a single pointer, null when empty), holding its ODF data
//! ([`odf`]), the VSDX elements it had no slot for, and unknown content of a newer `.diacraft`. A **held value** ([`odf::Held`]) keeps an ODF construct
//! exactly beside the stand-in cells that approximate it and records the [`stamp::Stamp`] of those cells; a writer of that format uses it only while the
//! cells still match, so a held value never overrides a cell. The package-level data (parts, relationships, content-type rows, custom properties) is in
//! [`package`]. A model with none of this compares exactly as it did before.
//!
//! **Names are checked at the write that sets them** ([`check_page_name`]), never by a pass over the whole document: an imported
//! page with a blank name must not stop every other edit.
//!
//! **The language of a run** is a cell (`LangID`) that files hold as they wrote it. [`lang`] reads a language tag by the syntax of RFC 5646 ([`lang::parse`]) and says
//! what a value means in a drawing ([`lang::effective`]: a language, no language, or the document's own), so that the renderer, the proofer and AutoCorrect read one
//! answer; a value is never rewritten by reading it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod alt_text;
mod catalogue;
mod cell;
mod comments;
mod diacraft_data;
mod document;
pub mod lang;
mod name;
pub mod odf;
pub mod package;
mod page_names;
mod part;
mod places;
mod raw;
mod resolve;
pub mod root_text;
mod shape;
mod shared;
mod sheet;
mod sheet_ref;
mod snap_words;
pub mod stamp;
mod string_unit;
pub use string_unit::units_are_ours_to_state;
mod theme_data;
mod value;
mod window_flags;
pub mod work;
mod write;

#[cfg(kani)]
mod kani_proofs;

pub use catalogue::{
    CellClass, CellInfo, DISPLAY_ORDER, DisplaySection, Family, Keying, SECTION_NAMES, SECTION_SPELLINGS, SectionKind, SectionRules, SheetKind,
    cell_info, direct_cell_spelling, display_group, display_section, ix_to_row_number, keying, listed_cells, next_row_number, row_number_to_ix,
    rows_inherit_type, section_class, section_for_spelling, section_rules, section_spelling,
};
pub use cell::{Cell, CellExtra, FormulaSlot, INHERIT_MARKER, NO_FORMULA_MARKER, Origin};
pub use comments::{
    Comment, CommentAuthor, CommentError, Comments, MAX_COMMENT_AUTHORS, MAX_COMMENT_TEXT_BYTES, MAX_COMMENTS, MAX_COMMENTS_TEXT_BYTES, NewComment,
};
pub use diacraft_data::{
    DIACRAFT_URN, DIACRAFT_URN_LEGACY, DIACRAFT_VERSION, PAGE_BREAKS_CHILD, current_namespace, declare_inherited_namespace, is_diacraft_urn,
    use_current_namespace,
};
pub use diacraft_formula::Formula;
pub use document::{ColorEntry, DocProps, Document, FaceName};
pub use name::{CellName, SectionName};
pub use odf::{
    DefFamily, Definition, Held, HeldGroup, HeldStyleValue, KeptStyle, MAX_DEFINITIONS, MAX_LIST_LEVELS, OdfDoc, OdfListLevel, OdfPage, OdfParagraph,
    OdfRow, OdfRun, OdfShape, OdfStyle, OdfText, OdfTextStyle, TextGroup,
};
pub use package::{ContentTypeRow, CustomProp, DIACRAFT_APPLICATION, Ext, ExtData, Format, Kept, MAX_PARTS, Part, PartRole, Rel, VsdxHeld, VsdxRest};
pub use page_names::{is_page_and_digits, page_name_key};
pub use part::{Connect, Connects, Master, Page, SchemeId, Style, Theme};
pub use places::{Counted, cell_names, cells_changed, owner_places_after, owner_places_after_where};
pub use raw::{Passthrough, Placed, RawAttrs, RawElement, RawNode};
pub use resolve::{
    DefaultSpec, EffectiveRow, Located, MAX_STYLE_STEPS, MasterStep, Member, Rebind, Resolved, Resolver, ResolverStats, RowShape, Source, Warnings,
    resolve,
};
pub use shape::{ForeignData, MAX_SHAPE_DEPTH, Media, Shape, ShapeList, ShapeType, Text, TextPart};
pub use shared::{CHUNK, Iter as SharedIter, OutOfRange, SharedVec};
pub use sheet::{
    CellAddr, Cells, Links, Lookup, MAX_CELLS_PER_CONTAINER, MAX_NAME_BYTES, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET, ROW_SCAN_LIMIT, Row,
    RowKey, Rows, Section, SectionKey, Sheet,
};
pub use sheet_ref::{Owner, ShapeRef, SheetRef};
pub use snap_words::{SnapWords, is_other_namespace, namespace_attribute};
pub use stamp::{
    Digest, Stamp, stamp_all, stamp_cells, stamp_row, stamp_rows_named, stamp_section, stamp_sections, stamp_text_group, stamp_text_row,
};
pub use theme_data::{
    ColourVariant, EffectEntry, FillEntry, FillProps, FontProps, GradientEntry, GradientKind, LineEntry, LineEx, MAX_THEME_ENTRIES,
    MAX_THEME_MODIFIERS, MAX_THEME_STOPS, ShadowEntry, StyleLists, StyleVariant, ThemeBase, ThemeColour, ThemeEntries, ThemeStop, VarStyle,
};
pub use value::{Reading, Value, ValueKind};
pub use window_flags::{WindowFlags, with_drawing_window};
pub use write::{CellEdit, CellWrite, WriteOrigin, WriteOutcome};

// The caps of a document. Each is a project rule, not a number from a spec or a file: gives
// the nesting depth (200) and nothing for pages, masters, styles or shapes, so the others are ours and the sample-file run of M2's Task 5
// confirms them against real files. A cap holds at each door a document enters: the reader of a file ([`Document::check_limits`] is its
// check) and the commands (`Document::add_page`, `add_style` and `add_master`). A document built in memory, by a test, may be past one,
// and everything in this crate still reads it without a panic.

/// Most pages a document holds. A page is cheap, but a history of whole-document snapshots keeps a list of them for each step, so the
/// list is bounded. A project rule (see above).
pub const MAX_PAGES: usize = 1_000;

/// Most styles a document holds: 65,536. A project rule (see above): real files hold about twenty each, and 38,092 across the census's
/// 1,937 files.
pub const MAX_STYLES: usize = 65_536;

/// Most masters a document holds: 65,536. A project rule (see above): gives 1,925 parts at the most, and a master is
/// a part.
pub const MAX_MASTERS: usize = 65_536;

/// Most shapes in one page part or master part, nested ones counted: 1,000,000. A project rule (see above).
pub const MAX_SHAPES_PER_PAGE: usize = 1_000_000;

/// Most `ColorEntry` rows in the colour table: 4,096. A project rule (see above): the fixed palette holds 24 colours and a table adds the
/// few a drawing defines. A lookup of an entry is a scan of this many at the most.
pub const MAX_COLOR_ENTRIES: usize = 4_096;

/// Longest page name the model holds, in characters: what a file may give a page.
pub const MAX_NAME_CHARS: usize = 255;

/// Longest name a command gives a page, in characters: **a page name has at most 31 characters** (the page setup dialog's Page Properties tab). A name a file holds stays as it is,
/// up to [`MAX_NAME_CHARS`]; [`check_given_page_name`] is the rule of the commands that name a page.
pub const MAX_PAGE_NAME_CHARS: usize = 31;

/// What can be wrong with a page, a name, a write or a document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocError {
    #[error("a page name must not be empty")]
    EmptyName,
    #[error("a page name must be at most {max} characters", max = MAX_NAME_CHARS)]
    NameTooLong,
    /// A name a command gave a page that has more than [`MAX_PAGE_NAME_CHARS`] characters.
    #[error("a page name must be at most {max} characters", max = MAX_PAGE_NAME_CHARS)]
    PageNameTooLong,
    #[error("a page name must not contain control characters")]
    ControlCharacter,
    /// A name a command gave a page that is "page" and digits (`page1`): programs that open the drawing rename such a page ([`is_page_and_digits`]).
    #[error("a page name must not be \"page\" and digits (page1, Page2), which programs that open the drawing rename: give the page another name")]
    PageNameReserved,
    /// A name a command gave a page that another page has, compared without regard to case ([`page_name_key`]).
    #[error("another page has this name, and names are compared without regard to case: give the page another name")]
    PageNameTaken,
    #[error("no page has id {0}")]
    NoSuchPage(u32),
    #[error("a document holds at most {max} pages", max = MAX_PAGES)]
    TooManyPages,
    #[error("no page ids are left")]
    IdsExhausted,
    #[error("page id {0} is used by more than one page")]
    DuplicatePageId(u32),
    /// A sheet that is not in the document: a page, master, style or shape that does not exist.
    #[error("there is no such sheet in the document")]
    NoSuchSheet,
    /// A name that is empty or longer than [`MAX_NAME_BYTES`]: the kind of name is given.
    #[error("{0} must not be empty or longer than {max} bytes", max = MAX_NAME_BYTES)]
    BadName(&'static str),
    /// A write that would pass a limit of the model, or a document past one.
    #[error("a document cannot hold more than {max} {what}")]
    Limit { what: &'static str, max: usize },
    /// A write into a row or section that is deleted here (`Del`): it would be lost, since nothing in a deleted row is read
    /// Write to the row to bring it back first. Our rule: no spec settles it.
    #[error("the {0} is deleted here (Del), so a write into it would be lost; bring it back first")]
    Deleted(&'static str),
    /// An edit other than a cached result under [`WriteOrigin::Evaluation`]: that origin is not stopped by a guard, so only a cached result
    /// (which leaves the formula as it is) may use it. Our rule: no spec settles it.
    #[error("an evaluation writes cached results only: any other edit under that origin would remove a guard")]
    EvaluationEdit,
}

/// What is wrong with a piece of text a person reads or types as a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextProblem {
    #[error("must not be empty")]
    Empty,
    #[error("must be at most {max} characters", max = MAX_NAME_CHARS)]
    TooLong,
    #[error("must not contain control characters")]
    ControlCharacter,
}

impl From<TextProblem> for DocError {
    fn from(problem: TextProblem) -> DocError {
        match problem {
            TextProblem::Empty => DocError::EmptyName,
            TextProblem::TooLong => DocError::NameTooLong,
            TextProblem::ControlCharacter => DocError::ControlCharacter,
        }
    }
}

/// The one rule for text that is shown to people as a name, which page names and the names of undo
/// steps both follow: not empty or blank, at most [`MAX_NAME_CHARS`] characters, no control
/// characters (a newline, a NUL or an escape in a name would reach menus, logs and terminals).
/// Surrounding spaces are allowed and kept.
pub fn check_text(text: &str) -> Result<(), TextProblem> {
    if text.trim().is_empty() {
        return Err(TextProblem::Empty);
    }
    // One past the limit is enough to know the text is too long, however long it is.
    if text.chars().take(MAX_NAME_CHARS + 1).count() > MAX_NAME_CHARS {
        return Err(TextProblem::TooLong);
    }
    if text.chars().any(char::is_control) {
        return Err(TextProblem::ControlCharacter);
    }
    Ok(())
}

/// Checks a page name by [`check_text`]: the rule of the model, which holds the name of any file (at most [`MAX_NAME_CHARS`] characters). [`Document::add_page`],
/// [`Document::rename_page`] and the audit use it, so that an importer can name a page as its file does.
pub fn check_page_name(name: &str) -> Result<(), DocError> {
    check_text(name).map_err(DocError::from)
}

/// Checks the name a command gives a page: the rule of [`check_page_name`], **at most [`MAX_PAGE_NAME_CHARS`] characters**, counted as characters and not bytes, and **not "page" and
/// digits** ([`is_page_and_digits`]). The commands that name a page (`page.add`, `page.rename`, `page.duplicate`) use it before they write, and
/// [`Document::check_free_page_name`] adds that no other page has the name; a reader does not, so a name a file holds stays as it is.
pub fn check_given_page_name(name: &str) -> Result<(), DocError> {
    // One past the limit is enough to know the name is too long, however long it is.
    if name.chars().take(MAX_PAGE_NAME_CHARS + 1).count() > MAX_PAGE_NAME_CHARS {
        return Err(DocError::PageNameTooLong);
    }
    check_page_name(name)?;
    if is_page_and_digits(name) {
        return Err(DocError::PageNameReserved);
    }
    Ok(())
}
