//! The `Theme` style: the style sheet a shape follows to take its values from the theme (and its kin).
//!
//! **What it holds**, the same as the style of the same name in files does: each format cell the theme answers holds the call `THEMEVAL()` with
//! the marker `Themed` as its value (the host cell's own property), so a shape that follows the style
//! asks the theme each time it draws; the ten rows of `FillGradient` hold the same for the stops of a gradient entry; and the scheme cells hold
//! 65534, which sends a shape to its page's cells. The Quick Style cells say which entry of the theme's lists a shape takes:
//! **100 in all four matrices and in the fill, line and shadow colours** (the first style of the page's style variant and the first colour of its
//! colour variant), 0 for the font colour (`dk1`). These numbers are ours: no spec says what a new shape's Quick Style cells hold, and the choice
//! is what makes a variant one click. A style that a file already holds under the name `Theme` is used as it is.
//!
//! **No theme.** With the page's scheme cells at 0 the same cells answer with the root style's, so a shape that follows this
//! style and a page with no theme draw as a shape that follows no style does. That is what "No theme" relies on.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Links, Row, Section, Sheet, Style, Value};

use crate::draw::Problem;

/// The `NameU` and `Name` of the style.
pub const THEME_STYLE_NAME: &str = "Theme";

/// The cells that hold the marker, directly under the style: the cells of the line, fill, shadow and gradient families that
/// `calc`'s `THEMEVAL` answers from a theme (`theme_names.rs`). `FillBkgndTrans` and `ShapeShdwObliqueAngle` are left out: the first is a name
/// the theme functions do not work out, and the second has no entry to read.
const THEMED: [&str; 26] = [
    "LineColor",
    "LineWeight",
    "LinePattern",
    "LineCap",
    "Rounding",
    "BeginArrow",
    "EndArrow",
    "BeginArrowSize",
    "EndArrowSize",
    "LineColorTrans",
    "FillForegnd",
    "FillBkgnd",
    "FillPattern",
    "FillForegndTrans",
    "FillGradientEnabled",
    "FillGradientDir",
    "FillGradientAngle",
    "RotateGradientWithShape",
    "ShdwForegnd",
    "ShdwForegndTrans",
    "ShdwPattern",
    "ShapeShdwType",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "ShapeShdwScaleFactor",
    "ShapeShdwBlur",
];

/// The cells that hold a number, with the number: the scheme cells send a shape to its page (65534), and the Quick Style cells say which entry.
const NUMBERS: [(&str, &str); 21] = [
    ("EnableLineProps", "1"),
    ("EnableFillProps", "1"),
    ("EnableTextProps", "1"),
    ("ThemeIndex", "65534"),
    ("ColorSchemeIndex", "65534"),
    ("EffectSchemeIndex", "65534"),
    ("ConnectorSchemeIndex", "65534"),
    ("FontSchemeIndex", "65534"),
    ("EmbellishmentIndex", "65534"),
    ("VariationColorIndex", "65534"),
    ("VariationStyleIndex", "65534"),
    ("QuickStyleLineMatrix", "100"),
    ("QuickStyleFillMatrix", "100"),
    ("QuickStyleEffectsMatrix", "100"),
    ("QuickStyleFontMatrix", "100"),
    ("QuickStyleLineColor", "100"),
    ("QuickStyleFillColor", "100"),
    ("QuickStyleShadowColor", "100"),
    ("QuickStyleFontColor", "0"),
    ("QuickStyleType", "0"),
    ("QuickStyleVariation", "0"),
];

/// The most gradient stops a style row list holds (stops 1 to 10).
const STOPS: u32 = 10;

fn themed() -> Cell {
    Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed")))
}

fn row_of(ix: u32, names: &[&str]) -> Row {
    let mut row = Row { ix: Some(ix), ..Row::default() };
    for name in names {
        row.cells.insert(CellName::new(name), themed());
    }
    row
}

/// What a new `Theme` style is based on, which the drawing's file decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    /// The root style, through all three links: a drawing that is saved as a VSDX file, which holds the style as files of that format do.
    Root,
    /// **The default graphic style of the file** a drawing from an `.odg` was read from, [`DEFAULT_GRAPHIC_STYLE_NAME`], when the file has one, and the root when it has not.
    /// In an `.odg` the `Theme` style is an empty common style that another program draws a shape with as it draws a style that has no parent: with its own defaults, which for
    /// LibreOffice are a serif face, where a shape that follows nothing is drawn with `standard`'s sans face (a black-box probe of LibreOffice 26.8.1.1, 2026-11-16). The style
    /// names it as its parent, so the shapes that follow the theme are drawn with the face the drawing's own shapes are. The model has the style based on it too (the links),
    /// so the file a session of its own reads is the style sheets the session holds, and a second save is no edit.
    ///
    /// **A file with none** (a hand-made one, and the blank package a new drawing is made from) gets the style on the root, and the `.odg` names no parent for it: nothing
    /// is invented. A program that draws the style as an empty common style then draws the shapes that follow it with its own defaults (LibreOffice: a serif face and its own
    /// blue, measured). Naming `standard` anyway would be a parent the file does not hold, and writing an empty `standard` would be worse: LibreOffice takes the file's empty
    /// style over its own and draws every shape of the drawing in the serif (both probed, 2026-11-16).
    FilesDefault,
}

/// The `NameU` of the style an `.odg` made by a drawing program has as the default graphic style that every shape's own style names as its parent (`style:default-style`, the
/// default the standard gives a family, has no name to be a parent by). It is **the name as the file writes it**: LibreOffice writes `standard` (black-box probe of 26.8.1.1,
/// 2026-11-16, a project rule: the standard names none). The first style sheet with that `NameU`: the model keeps no family of a style sheet, and a drawing program does
/// not write a paragraph style of the name.
pub const DEFAULT_GRAPHIC_STYLE_NAME: &str = "standard";

/// The `ID` of the default graphic style of the file ([`DEFAULT_GRAPHIC_STYLE_NAME`]): the first style sheet but the root that has the name, or `None`.
pub fn default_graphic_style_id(doc: &Document) -> Option<u32> {
    doc.styles.iter().find(|s| s.id != 0 && s.name_u.as_deref() == Some(DEFAULT_GRAPHIC_STYLE_NAME)).map(|s| s.id)
}

/// A new `Theme` style with this `ID`, based on the style `parent` (the root, 0, or the file's default graphic style) through all three links.
fn make(id: u32, parent: u32) -> Style {
    let mut sheet = Sheet { links: Links { line: Some(parent), fill: Some(parent), text: Some(parent) }, ..Sheet::default() };
    for (name, value) in NUMBERS {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
    for name in THEMED {
        sheet.cells.insert(CellName::new(name), themed());
    }
    let mut gradient = Section::new("FillGradient", None);
    for ix in 0..STOPS {
        gradient.rows.push(Arc::new(row_of(ix, &["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"])));
    }
    let mut character = Section::new("Character", None);
    character.rows.push(Arc::new(row_of(0, &["Color", "Font"])));
    sheet.sections.push(Arc::new(gradient));
    sheet.sections.push(Arc::new(character));
    Style { id, name: Some(Arc::from(THEME_STYLE_NAME)), name_u: Some(Arc::from(THEME_STYLE_NAME)), sheet: Arc::new(sheet), ..Style::default() }
}

/// The `ID` of the style a shape follows to take the theme's values: the first style whose `NameU` is `Theme`.
pub fn theme_style_id(doc: &Document) -> Option<u32> {
    doc.styles.iter().find(|s| s.name_u.as_deref() == Some(THEME_STYLE_NAME)).map(|s| s.id)
}

/// [`theme_style_id`], making the style when the document has none (based as `base` says), and **filling it in when it has none of what makes it the `Theme` style**: a file
/// another program saved keeps the empty common style the first save made and drops the document file that held its cells, so the style the drawing finds by name has no cell,
/// and a shape that follows it follows nothing (`theme.apply` answered that it added the theme and changed no look). Its links stay what
/// they are. Gives the style's `ID` and whether the style was made or filled in. Fails at the style cap, with the document as it was.
pub(super) fn ensure_theme_style(doc: &mut Document, base: Base) -> Result<(u32, bool), Problem> {
    if let Some(at) = doc.styles.iter().position(|s| s.name_u.as_deref() == Some(THEME_STYLE_NAME)) {
        let Some(found) = doc.styles.get_mut(at) else { return Err(Problem::failed("the Theme style is not there")) };
        if !found.sheet.cells.is_empty() || !found.sheet.sections.is_empty() {
            return Ok((found.id, false));
        }
        // The cells the style holds when it is made, under the links it has.
        let mut filled = make(found.id, 0).sheet;
        Arc::make_mut(&mut filled).links = found.sheet.links;
        Arc::make_mut(found).sheet = filled;
        return Ok((found.id, true));
    }
    let parent = match base {
        Base::Root => 0,
        Base::FilesDefault => default_graphic_style_id(doc).unwrap_or(0),
    };
    // One past the highest `ID`; style 0 is the root, so the first free is 1.
    let id = doc
        .styles
        .iter()
        .map(|s| s.id)
        .max()
        .map_or(Some(1), |highest| highest.checked_add(1))
        .ok_or_else(|| Problem::failed("no style ID is left"))?;
    doc.add_style(make(id, parent)).map_err(|e| Problem::failed(e.to_string()))?;
    Ok((id, true))
}
