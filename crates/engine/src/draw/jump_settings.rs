//! The line jump settings of a page and of a connector, and the command that writes them (in `jumps.rs`).

use diacraft_doc::{CellAddr, CellEdit, Document, SheetRef, Value};

use super::Problem;
use super::jumps::{DEFAULT_FACTOR, Settings, refresh};
use super::read::Reader;
use super::scan;
use crate::stencil::number_text;

/// The page's line jump settings, for a window to show: a cell found nowhere is the root's value (`LineJumpCode` 1).
pub fn line_jump_settings(doc: &Document, page: u32) -> LineJumpSettings {
    let s = Settings::of_page(&Reader::new(doc), page);
    LineJumpSettings { code: s.code, style: s.style, factor_x: s.factor.0, factor_y: s.factor.1, dir_x: s.dir.0, dir_y: s.dir.1 }
}

/// What a page says of line jumps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineJumpSettings {
    /// `LineJumpCode`: which of two crossing connectors takes the jump (0 none, 1 horizontal lines, 2 vertical lines, 3 last routed, 4 top of the
    /// drawing order, 5 bottom).
    pub code: u32,
    /// `LineJumpStyle`: 0 and 1 arc, 2 gap, 3 square, 4 to 9 two to seven sides.
    pub style: u32,
    /// `LineJumpFactorX` and `LineJumpFactorY`: the size, as a multiple of the page's line to line spacing.
    pub factor_x: f64,
    pub factor_y: f64,
    /// `PageLineJumpDirX` (0 and 1 up, 2 down) and `PageLineJumpDirY` (0 and 1 left, 2 right).
    pub dir_x: u32,
    pub dir_y: u32,
}

/// What a command changes of the jump settings: each field that is set. `show` is the Connectors menu's Show Line Jumps (our reading: on makes the
/// page's code 1 if it is 0 and leaves another code as it is; off makes it 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JumpChange {
    pub show: Option<bool>,
    pub code: Option<u32>,
    pub style: Option<u32>,
    pub dir_x: Option<u32>,
    pub dir_y: Option<u32>,
    pub factor_x: Option<f64>,
    pub factor_y: Option<f64>,
}

fn check(name: &str, value: Option<u32>, most: u32) -> Result<(), Problem> {
    match value {
        Some(v) if v > most => Err(Problem::bad(format!("`{name}` must be from 0 to {most}"))),
        _ => Ok(()),
    }
}

/// Writes the jump settings of the page (with `ids` None) or the overrides of connectors `ids` (`ConLineJumpCode`, `ConLineJumpStyle`,
/// `ConLineJumpDirX` and `ConLineJumpDirY`: `show` and the sizes are the page's alone), and rebuilds the jumps of every connector on the page
/// → whether the document changed.
pub fn set_line_jumps(doc: &mut Document, page: u32, ids: Option<&[u32]>, change: &JumpChange) -> Result<bool, Problem> {
    let before = doc.clone();
    super::require_page(doc, page)?;
    check("code", change.code, 5)?;
    check("style", change.style, 9)?;
    check("dir_x", change.dir_x, 2)?;
    check("dir_y", change.dir_y, 2)?;
    for (name, value) in [("factor_x", change.factor_x), ("factor_y", change.factor_y)] {
        if value.is_some_and(|v| !(v.is_finite() && (0.0..=10.0).contains(&v))) {
            return Err(Problem::bad(format!("`{name}` must be a number from 0 to 10")));
        }
    }
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    {
        let reader = Reader::new(doc);
        let mut put = |sheet: &SheetRef, name: &str, value: f64, default: f64| {
            let addr = CellAddr::direct(name);
            let held = if reader.is_set(sheet, &addr) { reader.number(sheet, name) } else { default };
            if (held - value).abs() > 1.0e-12 || !reader.is_set(sheet, &addr) {
                writes.push((sheet.clone(), addr, CellEdit::Value(Value::from_text(&number_text(value)))));
            }
        };
        match ids {
            None => {
                let sheet = SheetRef::Page(page);
                let now = Settings::of_page(&reader, page).code;
                let code = match (change.code, change.show) {
                    (Some(code), _) => Some(code),
                    (None, Some(true)) => Some(if now == 0 { 1 } else { now }),
                    (None, Some(false)) => Some(0),
                    (None, None) => None,
                };
                for (name, value, default) in [
                    ("LineJumpCode", code.map(f64::from), 1.0),
                    ("LineJumpStyle", change.style.map(f64::from), 0.0),
                    ("PageLineJumpDirX", change.dir_x.map(f64::from), 0.0),
                    ("PageLineJumpDirY", change.dir_y.map(f64::from), 0.0),
                    ("LineJumpFactorX", change.factor_x, DEFAULT_FACTOR),
                    ("LineJumpFactorY", change.factor_y, DEFAULT_FACTOR),
                ] {
                    if let Some(value) = value {
                        put(&sheet, name, value, default);
                    }
                }
            }
            Some(ids) => {
                if change.show.is_some() || change.factor_x.is_some() || change.factor_y.is_some() {
                    return Err(Problem::bad("`show`, `factor_x` and `factor_y` belong to the page: leave `ids` out"));
                }
                let (_, found) = scan::named(doc, page, ids)?;
                for (id, at) in &found {
                    let sheet = SheetRef::Shape(at.clone());
                    if !reader.is_one_d(&sheet) {
                        return Err(Problem::bad(format!("shape {id} is not a connector")));
                    }
                    for (name, value) in [
                        ("ConLineJumpCode", change.code),
                        ("ConLineJumpStyle", change.style),
                        ("ConLineJumpDirX", change.dir_x),
                        ("ConLineJumpDirY", change.dir_y),
                    ] {
                        if let Some(value) = value {
                            put(&sheet, name, f64::from(value), 0.0);
                        }
                    }
                }
            }
        }
    }
    super::write_cells(doc, writes)?;
    refresh(doc, &before, page, &[], true)?;
    Ok(*doc != before)
}
