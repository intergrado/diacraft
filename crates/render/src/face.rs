//! The face a run of a shape's text is set in: read from the cells in one place, for the renderer that draws the run and for whatever must say what
//! the renderer draws it in (the window's Font box, `shape.formatting`).
//!
//! **The rule**: the `Font` cell of the run's
//! `Character` row, found through the shape's own cells, its master and the style chain, the theme's font when the cell holds the marker (`calc`
//! runs the cell's own `THEMEVAL` call), and for a shape with no style link the root style's cell. A name that is empty or the number 0 says no font
//! is specified, and the run then takes the **root style's** `Font`, read the same way. When that names none either, the run
//! names no face: the layout asks the font database for the default of its kind (`FontDb::resolve("")`, `FontDb::default_family`).
//!
//! The name is the document's, or the theme's, as it is written: Calibri stays Calibri where Carlito draws it. Which face a name is drawn with is the
//! font database's (`FontDb::resolve`: the name, then its stand-in, then the default), and not read here.

use diacraft_doc::{CellAddr, Document, RowKey, SectionKey, SheetRef};

use crate::read::{Cx, eval_ctx};

/// A cell of a row of a section.
pub(crate) fn row_cell(section: &str, ix: u32, name: &str) -> CellAddr {
    CellAddr { section: Some(SectionKey::new(section, None)), row: Some(RowKey::Ix(ix)), cell: diacraft_doc::CellName::new(name) }
}

/// A face name cell: `0` and empty say none is specified.
pub(crate) fn face_name(cx: &Cx<'_, '_>, at: &SheetRef, addr: &CellAddr) -> String {
    let name = cx.text_at(at, addr).map(|t| t.to_string()).unwrap_or_default();
    if name == "0" { String::new() } else { name }
}

/// The face the run `ix` of the shape `at` names for its text: its `Font` cell, and when that names none the root style's. Empty when no
/// cell names a face. **The one function the renderer's layout and the readers of [`FaceReader`] both use.**
pub(crate) fn run_font(cx: &Cx<'_, '_>, at: &SheetRef, ix: u32) -> String {
    let cell = row_cell("Character", ix, "Font");
    let font = face_name(cx, at, &cell);
    if font.is_empty() {
        // `Font` of 0 takes the root style's.
        return face_name(cx, &SheetRef::Style(0), &cell);
    }
    font
}

/// Reads the faces the renderer sets text in, from a document, as the renderer reads them. Made by [`with_face_reader`].
pub struct FaceReader<'x, 'a, 'c> {
    cx: &'x Cx<'a, 'c>,
}

impl FaceReader<'_, '_, '_> {
    /// The face the cells of the `Character` row `run` of `shape` name for its text, as the renderer reads them (the module's rule): `None` when no
    /// cell names one, and the renderer then draws the font database's default (`FontDb::default_family` names it). A shape that is not in the
    /// document reads as one with no cells, and names none.
    pub fn named_font(&self, shape: &diacraft_doc::ShapeRef, run: u32) -> Option<String> {
        let name = run_font(self.cx, &SheetRef::Shape(shape.clone()), run);
        (!name.is_empty()).then_some(name)
    }
}

/// Runs `read` with a [`FaceReader`] of `doc`. The theme's answers are kept for as long as the reader lives, so many reads share them; the steps
/// `calc` may spend on them are the renderer's own bound for a frame.
pub fn with_face_reader<R>(doc: &Document, read: impl FnOnce(&FaceReader<'_, '_, '_>) -> R) -> R {
    let mut ctx = eval_ctx();
    let cx = Cx::new(doc, &mut ctx);
    read(&FaceReader { cx: &cx })
}
