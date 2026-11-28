//! A shape's accessible title and description: `svg:title` is the row `visAltText` of the shape's `User` section and
//! `svg:desc` the row `visAltDescription`.
//!
//! **What is read.** The first plain element of each kind among the shape's own children: character data and nothing else (no attribute, no element, comment or processing instruction in it), and
//! at least one character. Its text is the row's `Value` as it is, with every space, line break and tab (a title of spaces is a row of spaces, which R13 counts as no alt text), in the
//! form the files hold (`diacraft_doc::alt_text::standard_row`). **An element that is anything else stays kept markup** as it was before this mapping: an empty one, one with an attribute or a
//! child, and the second of its kind. **An element that became a row is not kept**: the writer makes it again from the row, so a row and its element cannot disagree.
//!
//! **What DiaCraft's own data says over it.** A shape whose style carries a `User` section (DiaCraft's data: the rows the markup cannot say, as a formula in the title) is read with that section
//! over the rows the markup gave. The elements are what another program edits, though, and LibreOffice keeps DiaCraft's attributes through an edit of a title: so where the carried section
//! holds a row whose text is not the element's, **the element's text stands** ([`Alt::reconcile`]), as a row of the form the files hold, and an element another program took out empties the row.
//! A carried section that holds no row for a text and an element that holds one is left to DiaCraft's data: it is how a shape whose text comes from its master is carried (`Gone`).

use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::alt_text::{DESCRIPTION_ROW, SECTION, TITLE_ROW, standard_row};
use diacraft_doc::{Section, SectionKey, Sheet};

use super::Reader;
use super::names::{Names, Ns};
use crate::error::OdgError;

/// What the first plain `svg:title` and `svg:desc` among an element's children say.
#[derive(Default)]
pub(crate) struct Alt<'t> {
    pub title: Option<(&'t El, String)>,
    pub description: Option<(&'t El, String)>,
}

/// Whether `el` holds character data and nothing else, and some of it, with no attribute but namespace declarations: the text.
fn plain_text(el: &El) -> Option<String> {
    if el.attrs.iter().any(|(k, _)| k != "xmlns" && !k.starts_with("xmlns:")) {
        return None;
    }
    if !el.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_))) {
        return None;
    }
    let text = el.text();
    (!text.is_empty()).then_some(text)
}

/// The elements of `el`'s own children that are its title and its description.
pub(crate) fn find<'t>(names: &mut Names<'t>, el: &'t El) -> Alt<'t> {
    let mut alt = Alt::default();
    for child in el.elements() {
        let mark = names.enter(child);
        let which = match names.element(child) {
            (Ns::Svg, "title") => Some(true),
            (Ns::Svg, "desc") => Some(false),
            _ => None,
        };
        names.leave(mark);
        let Some(is_title) = which else { continue };
        let slot = if is_title { &mut alt.title } else { &mut alt.description };
        // The second of a kind (the schema allows one) and one that is not plain are kept as they were.
        if slot.is_some() {
            continue;
        }
        if let Some(text) = plain_text(child) {
            *slot = Some((child, text));
        }
    }
    alt
}

/// The rows' texts, by row name.
fn rows<'a>(alt: &'a Alt<'_>) -> [(&'static str, Option<&'a str>); 2] {
    [(TITLE_ROW, alt.title.as_ref().map(|(_, t)| t.as_str())), (DESCRIPTION_ROW, alt.description.as_ref().map(|(_, t)| t.as_str()))]
}

impl Alt<'_> {
    /// Whether `child` is one of the elements that became a row.
    pub(crate) fn took(&self, child: &El) -> bool {
        [&self.title, &self.description].into_iter().flatten().any(|(el, _)| std::ptr::eq(*el, child))
    }

    /// Whether there is nothing to give.
    pub(crate) fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none()
    }

    /// Adds the rows to the sheet's `User` section (made if the sheet has none), after the rows it has. Charged to the read.
    pub(crate) fn put(&self, r: &mut Reader, sheet: &mut Sheet) -> Result<(), OdgError> {
        if self.is_empty() {
            return Ok(());
        }
        let key = SectionKey::new(SECTION, None);
        if sheet.section(&key).is_none() {
            r.take_items(super::cost::ALT_SECTION)?;
            sheet.sections.push(Arc::new(Section::new(SECTION, None)));
        }
        for (name, text) in rows(self) {
            let Some(text) = text else { continue };
            r.take_items(super::cost::ALT_ROW)?;
            r.take_cells(2)?;
            r.take_bytes(text.len())?;
            let section = sheet.section_mut(&key).map_err(OdgError::Model)?;
            section.rows.push(Arc::new(standard_row(name, text)));
        }
        Ok(())
    }

    /// After DiaCraft's data was put over the rows: the element's text stands where the carried row holds another (see the module documentation). A carried section that has no row for
    /// a text the element holds is given it (a title added to a shape in another program, or by a save into the file, after the section was carried); a carried row whose element is
    /// gone is emptied; a row that says the element's text keeps the form it was carried in (a formula in a title). **No `User` section after DiaCraft's data is none to reconcile**: that
    /// is how a shape that takes its texts from its master is carried (`Gone`), and the elements it was written with are its master's.
    pub(crate) fn reconcile(&self, r: &mut Reader, sheet: &mut Sheet) -> Result<(), OdgError> {
        let key = SectionKey::new(SECTION, None);
        for (name, native) in rows(self) {
            let Some(section) = sheet.section(&key) else { return Ok(()) };
            let held = section.rows.iter().position(|row| row.name.as_deref() == Some(name));
            let have =
                held.and_then(|at| section.rows.get(at)).and_then(|row| row.cells.get("Value")).and_then(|c| c.value.as_ref()).map(|v| v.text());
            let want = native.unwrap_or("");
            if (held.is_none() && native.is_none()) || (held.is_some() && have.unwrap_or("") == want) {
                continue;
            }
            r.take_items(super::cost::ALT_ROW)?;
            r.take_cells(2)?;
            r.take_bytes(want.len())?;
            let section = sheet.section_mut(&key).map_err(OdgError::Model)?;
            let row = Arc::new(standard_row(name, want));
            match held.and_then(|at| section.rows.get_mut(at)) {
                Some(slot) => *slot = row,
                None => section.rows.push(row),
            }
        }
        Ok(())
    }
}
