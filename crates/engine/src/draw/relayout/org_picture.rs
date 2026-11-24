//! The picture on a position shape (ours): Org Chart, Picture, Insert Picture, Change Picture, Delete Picture and Hide Picture
//! (`OrgChartPictureInsertOne`, `OrgChartPictureChange`, `OrgChartPictureDelete`, `OrgChartPictureHide`; "Org chart picture on a
//! shape": "tab commands insert, change, delete or hide a picture in the selected position shape").
//!
//! **The picture is the position's own.** The specifications do not say what a shape with a picture holds, and a position is one shape, so the picture is **the shape's `ForeignData`** (the
//! renderer draws a picture of any shape that holds one, in the box of its `ImgOffsetX`, `ImgOffsetY`, `ImgWidth` and `ImgHeight`) and the shape keeps
//! its geometry, its text, its data and its glue: it moves, resizes, copies and is laid out as one shape. The cells are the shape's own, formulas of its `Height` and `Width` so that they
//! follow a resize:
//!
//! * the picture goes in a square at the left of the shape, a twelfth of the height in from the edges (84 hundredths of the height across), **the picture's proportions kept** inside it;
//! * the text block moves to the right of the square: `TxtWidth` is `Width-Height` (never less than a quarter of the width) and `TxtPinX` its middle;
//! * **Hide Picture** is the user cell `User.DiaCraftOrgPictureHidden` (1 hides): the box's size and the text block's formulas read it, so the picture is kept and the text takes the whole
//!   shape again; showing it is writing 0;
//! * **Delete Picture** takes the `ForeignData`, those cells and the user cell away: the shape is as it was.
//!
//! **Files.** A shape with `ForeignData` and a picture is kept by `.diacraft` and VSDX whatever its type; for `.odg` the tests in `tests/it/orgchart_files.rs` say what a save keeps.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellEdit, ForeignData, RawAttrs, RowKey, ShapeRef, ShapeType, SheetRef, Value as CellValue};

use super::super::pictures::{Info, media_for};
use super::super::settle::{settle, settle_cells};
use super::super::{Problem, shape_mut, write_cells};
use super::org::positions;
use crate::stencil::number_text;

/// The user cell that hides the picture.
pub const HIDDEN_ROW: &str = "DiaCraftOrgPictureHidden";

/// The share of the height the square of the picture is across, and the share each side of it is in from the shape's edge.
const SQUARE: f64 = 0.84;
const PAD: f64 = 0.08;

/// The cells a picture writes, by name: the box and the text block.
const CELLS: [&str; 6] = ["ImgOffsetX", "ImgOffsetY", "ImgWidth", "ImgHeight", "TxtWidth", "TxtPinX"];

fn hidden_addr() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(HIDDEN_ROW), "Value")
}

/// What a position's picture is now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PictureState {
    /// The shape holds a picture.
    pub has: bool,
    /// It is hidden.
    pub hidden: bool,
}

/// A formula of `Height` for the share `k`, with the value it gives at `height`.
fn of_height(k: f64, height: f64) -> Cell {
    Cell::with_formula(&format!("Height*{}", number_text(k)), Some(CellValue::from_text(&number_text(k * height)))).with_unit("IN")
}

/// `IF(User.DiaCraftOrgPictureHidden=1,0,<formula>)`: the cell is 0 when the picture is hidden.
fn unless_hidden(formula: &str, value: f64) -> Cell {
    Cell::with_formula(&format!("IF(User.{HIDDEN_ROW}=1,0,{formula})"), Some(CellValue::from_text(&number_text(value)))).with_unit("IN")
}

/// The picture of position `id` now.
pub fn state(doc: &diacraft_doc::Document, at: &ShapeRef) -> PictureState {
    let Some(shape) = doc.shape(at) else { return PictureState::default() };
    let has = shape.foreign.as_ref().is_some_and(|f| f.picture.is_some());
    let hidden = has
        && matches!(shape.sheet.lookup(&hidden_addr()), diacraft_doc::Lookup::Found(cell) if cell.value.as_ref().and_then(CellValue::number).is_some_and(|n| n != 0.0));
    PictureState { has, hidden }
}

/// The position `id` of `page`, which must be one.
fn position(doc: &diacraft_doc::Document, page: u32, id: u32) -> Result<ShapeRef, Problem> {
    positions(doc, page)?.into_iter().find(|p| p.id == id).map(|p| p.at).ok_or_else(|| {
        Problem::bad(format!("shape {id} of page {page} is not a position of an org chart (a shape dropped from the Organization Chart Shapes)"))
    })
}

/// What setting a picture did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureSet {
    /// The drawing held the same bytes already.
    pub shared: bool,
    /// The shape had a picture, which this one replaced.
    pub replaced: bool,
}

/// Puts a picture of `bytes` (which `info` says what they are) on position `id`, replacing the one it has (Insert Picture and Change Picture). The picture is shown, even when the one it
/// replaces was hidden. Refused, with the document as it was, for a shape that is no position and for pictures that would pass the drawing's budget (`media_for`).
pub fn set_picture(doc: &mut diacraft_doc::Document, page: u32, id: u32, info: &Info, bytes: Arc<[u8]>) -> Result<PictureSet, Problem> {
    let at = position(doc, page, id)?;
    let before = state(doc, &at);
    let (media, shared) = media_for(doc, info, bytes)?;
    // The shape's own size, to give the cells the values they have: the formulas are the truth, and the commit works them out again.
    let (width, height) = {
        let reader = super::super::read::Reader::new(doc);
        let sheet = SheetRef::Shape(at.clone());
        (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"))
    };
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err(Problem::failed(format!("position {id} has no size to put a picture in")));
    }
    // The picture's proportions kept inside the square: the longer side fills it.
    let aspect = (info.width / info.height).max(1.0e-3);
    let (fw, fh) = if aspect >= 1.0 { (SQUARE, SQUARE / aspect) } else { (SQUARE * aspect, SQUARE) };
    let (ox, oy) = (PAD + (SQUARE - fw) / 2.0, PAD + (SQUARE - fh) / 2.0);
    let sheet = SheetRef::Shape(at.clone());
    let text_width = (width - height).max(width * 0.25);
    let cells: Vec<(&str, Cell)> = vec![
        ("ImgOffsetX", of_height(ox, height)),
        ("ImgOffsetY", of_height(oy, height)),
        ("ImgWidth", unless_hidden(&format!("Height*{}", number_text(fw)), fw * height)),
        ("ImgHeight", unless_hidden(&format!("Height*{}", number_text(fh)), fh * height)),
        (
            "TxtWidth",
            Cell::with_formula(
                &format!("IF(User.{HIDDEN_ROW}=1,Width,MAX(Width-Height,Width*0.25))"),
                Some(CellValue::from_text(&number_text(text_width))),
            )
            .with_unit("IN"),
        ),
        (
            "TxtPinX",
            Cell::with_formula(
                &format!("IF(User.{HIDDEN_ROW}=1,Width*0.5,Height+TxtWidth*0.5)"),
                Some(CellValue::from_text(&number_text(height + text_width * 0.5))),
            )
            .with_unit("IN"),
        ),
    ];
    // The shape holds the picture and the box.
    {
        let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
        let attrs = RawAttrs::from_pairs([("ForeignType", info.format.foreign_type())]);
        shape.foreign = Some(Arc::new(ForeignData { attrs, picture: Some(media), ..ForeignData::default() }));
        // A shape of `Type` `Foreign` holds its picture in every format (a VSDX reader takes `ForeignData` of that type only); it keeps its geometry, text, data and glue.
        shape.kind = ShapeType::Foreign;
    }
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> =
        vec![(sheet.clone(), hidden_addr(), CellEdit::Cell(Cell::constant(CellValue::from_text("0"))))];
    let mut touched: Vec<(SheetRef, CellAddr)> = vec![(sheet.clone(), hidden_addr())];
    for (name, cell) in cells {
        writes.push((sheet.clone(), CellAddr::direct(name), CellEdit::Cell(cell)));
        touched.push((sheet.clone(), CellAddr::direct(name)));
    }
    write_cells(doc, writes)?;
    settle_cells(doc, page, touched)?;
    Ok(PictureSet { shared, replaced: before.has })
}

/// Takes position `id`'s picture away (Delete Picture): its `ForeignData`, the box, the text block's cells and the user cell. → whether it had one.
pub fn delete_picture(doc: &mut diacraft_doc::Document, page: u32, id: u32) -> Result<bool, Problem> {
    let at = position(doc, page, id)?;
    let held = doc.shape(&at).is_some_and(|s| s.foreign.is_some() || CELLS.iter().any(|c| s.sheet.cell(c).is_some()));
    if !held {
        return Ok(false);
    }
    let before = doc.clone();
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    shape.foreign = None;
    if shape.kind == ShapeType::Foreign {
        shape.kind = ShapeType::Shape;
    }
    let sheet = Arc::make_mut(&mut shape.sheet);
    for name in CELLS {
        sheet.cells.remove(name);
    }
    for section in sheet.sections.iter_mut() {
        if section.name.as_str() == "User" {
            let section = Arc::make_mut(section);
            section.rows.retain(|row| row.name.as_deref() != Some(HIDDEN_ROW));
        }
    }
    sheet.sections.retain(|s| !(s.name.as_str() == "User" && s.rows.is_empty() && s.cells.is_empty()));
    settle(doc, &before, page, &[id], &[])?;
    Ok(true)
}

/// Hides or shows position `id`'s picture (Hide Picture): the user cell, which the box and the text block read. → whether it changed. A shape with no picture is refused in words.
pub fn hide_picture(doc: &mut diacraft_doc::Document, page: u32, id: u32, hidden: bool) -> Result<bool, Problem> {
    let at = position(doc, page, id)?;
    let now = state(doc, &at);
    if !now.has {
        return Err(Problem::bad(format!("position {id} has no picture to hide or show")));
    }
    if now.hidden == hidden {
        return Ok(false);
    }
    let sheet = SheetRef::Shape(at);
    let value = CellValue::from_text(if hidden { "1" } else { "0" });
    write_cells(doc, vec![(sheet.clone(), hidden_addr(), CellEdit::Value(value))])?;
    settle_cells(doc, page, vec![(sheet, hidden_addr())])?;
    Ok(true)
}
