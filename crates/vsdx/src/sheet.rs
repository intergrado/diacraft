//! Sheets: the `Cell`, `Section`, `Row` and `Trigger` elements that every `PageSheet`, `Shape`, `StyleSheet` and `DocumentSheet` holds.
//!
//! **The codec is `diacraft-sheetxml`'s**: how a cell, a section and a row are read, what a hostile sheet does, and the caps and the
//! budgets, are written there, and its module documentation says them. This file joins that reader to this one: the reader's [`Notes`] are
//! what the codec's warnings go to, the document's budgets are the codec's (`Reader::sheets`), and an error of the codec is a
//! [`VsdxError::Limit`] with the same class word it always had.
//!
//! [`Notes`]: crate::report::Notes

use std::sync::Arc;

use diacraft_doc::Sheet;
use diacraft_opc::xml::El;
use diacraft_sheetxml::Cx;
pub(crate) use diacraft_sheetxml::{SheetTally, Took, style_link};

use crate::error::VsdxError;
use crate::ns::Scope;
use crate::reader::Reader;

impl Reader<'_> {
    /// Reads a `PageSheet` or a `DocumentSheet`: an element that is only a sheet. Its attributes are the links, and every other one in `raw`
    /// (`UniqueID`, and a document sheet's `Name`, `NameU`, `IsCustomName`, `IsCustomNameU`); its unknown children are in the sheet's own
    /// passthrough.
    pub(crate) fn read_sheet_only(&mut self, el: &El, sc: &Scope, part: &str) -> Result<Sheet, VsdxError> {
        let mut cx = Cx::new(&mut self.notes, part);
        Ok(self.sheets.read_sheet_only(&mut cx, el, sc)?)
    }

    /// The allocation a finished shape sheet is to be: the first equal one this read has met (`diacraft_sheetxml::SheetReader::share_sheet`), and what the copy was charged given back.
    pub(crate) fn share_sheet(&mut self, sheet: Sheet, tally: &SheetTally) -> Arc<Sheet> {
        self.sheets.share_sheet(sheet, tally)
    }

    /// Takes the sizes of a finished sheet for the report.
    pub(crate) fn note_sheet(&mut self, sheet: &Sheet) {
        self.sheets.note_sheet(sheet);
    }

    /// Reads a child of a sheet element if it is a `Cell`, a `Section` or a `Trigger`, putting it in `sheet`. `typed` is the number of
    /// typed children of the element so far, for where a kept node goes.
    pub(crate) fn sheet_child(
        &mut self,
        sheet: &mut Sheet,
        tally: &mut SheetTally,
        el: &El,
        sc: &Scope,
        part: &str,
        typed: usize,
    ) -> Result<Took, VsdxError> {
        let mut cx = Cx::new(&mut self.notes, part);
        Ok(self.sheets.sheet_child(&mut cx, sheet, tally, el, sc, typed)?)
    }
}
