//! A text box: the shape the Text tool makes (`stencil/text_box.rs` says what it holds and why it has no master).
//!
//! The box is added to the end of the page's list, so it is in front. Its `ID` is above every `ID` on the page and above the session's
//! high-water mark, as a dropped shape's is, and the page is walked once for it.

use std::sync::Arc;

use diacraft_doc::{Document, MAX_SHAPES_PER_PAGE};

use super::shapes::page_stats;
use super::text_edit::check_text;
use super::{Problem, checked, page_mut, require_page};
use crate::stencil::text_box_shape;

/// What a text box made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBoxInfo {
    /// The new shape's `ID` on the page.
    pub id: u32,
    /// Its size in the page's drawing units.
    pub width: f64,
    pub height: f64,
}

/// Adds a text box centred at (`x`, `y`) on a page, `width` by `height` (all in drawing units), holding `text` (empty for none). Its `ID` is
/// above `above` as well as above every `ID` on the page: the session's high-water mark (`Session::id_floor`), so that an `ID` that was a
/// shape's is never a new shape's. A size that is not more than 0, a place or size past [`super::MAX_COORD`], text a shape cannot hold and a full
/// page are refused, and the document is as it was.
pub fn add_text_box_above(
    doc: &mut Document,
    page: u32,
    (x, y): (f64, f64),
    (width, height): (f64, f64),
    text: &str,
    above: u32,
) -> Result<TextBoxInfo, Problem> {
    let (x, y) = (checked("x", x)?, checked("y", y)?);
    let (width, height) = (checked("width", width)?, checked("height", height)?);
    if width <= 0.0 || height <= 0.0 {
        return Err(Problem::bad("`width` and `height` must be more than 0"));
    }
    check_text(text)?;
    require_page(doc, page)?;
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let shape = text_box_shape(id, (x, y), (width, height), text);
    page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.shapes.push(Arc::new(shape));
    Ok(TextBoxInfo { id, width, height })
}
