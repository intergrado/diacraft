//! A container's settings: how it resizes, its margin, whether it is locked, where its heading sits and its style.
//!
//! The specifications name the settings of the Container Format tab (Margins, Fit to Contents, Automatic Resize with None, Expand as Needed and Always Fit to Contents, Heading
//! Style, Lock Container) and no cell a file holds them in. So they are `User` rows of ours, `User.DiaCraftContainer*`, on the container (`stencil/container.rs`), and a
//! container that has none of them (one a file holds) has the defaults: it expands as needed, with a margin of a quarter inch, unlocked, with no strip of ours.

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};

use super::page::Page;
use crate::draw::{Problem, shape_mut};
use crate::stencil::container::{HEADING_ROW, LOCK_ROW, MARGIN_ROW, RESIZE_ROW, STYLE_ROW, Side, find_style};

/// How a container follows its members' boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resize {
    /// It stays the size it is.
    None,
    /// It grows to hold a member that needs more room, and never shrinks.
    Expand,
    /// It is always the size that just holds its members.
    Fit,
}

impl Resize {
    pub fn number(self) -> u8 {
        match self {
            Resize::None => 0,
            Resize::Expand => 1,
            Resize::Fit => 2,
        }
    }

    pub fn from_number(n: f64) -> Resize {
        if (n - 0.0).abs() < 0.5 {
            Resize::None
        } else if (n - 2.0).abs() < 0.5 {
            Resize::Fit
        } else {
            Resize::Expand
        }
    }

    /// The word programs use: `none`, `expand`, `fit`.
    pub fn word(self) -> &'static str {
        match self {
            Resize::None => "none",
            Resize::Expand => "expand",
            Resize::Fit => "fit",
        }
    }

    pub fn from_word(word: &str) -> Option<Resize> {
        [Resize::None, Resize::Expand, Resize::Fit].into_iter().find(|r| r.word() == word)
    }
}

/// The thickness of the heading strip, in paper inches (ours).
pub const BAND_INCHES: f64 = 0.3;

/// The margin a container has when it holds none of its own, in paper inches.
pub const DEFAULT_MARGIN_INCHES: f64 = 0.25;

/// The presets of the Margins menu, in paper inches (the specification does not describe its choices: these are ours).
pub const MARGIN_PRESETS: [(&str, f64); 4] = [("none", 0.0), ("narrow", 0.1), ("medium", DEFAULT_MARGIN_INCHES), ("wide", 0.5)];

/// What a container's settings say, in the page's drawing units.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Settings {
    pub resize: Resize,
    pub margin: f64,
    pub locked: bool,
    pub heading: Side,
    /// The style's id, when the container is one made here.
    pub style: Option<String>,
    /// The heading strip's thickness: 0 for a container with no strip of ours.
    pub band: f64,
}

fn row(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

impl Settings {
    /// What the container `id` says (the defaults for a cell it has not). The reads are charged: a spent budget is an error and never the defaults.
    pub(super) fn read(page: &Page<'_>, id: u32) -> Result<Settings, Problem> {
        page.work.charge(8)?;
        let sheet = page.sheet(id);
        let number = |name: &str| sheet.as_ref().filter(|s| page.reader.is_set(s, &row(name))).map(|s| page.reader.addr_number(s, &row(name)));
        let style = sheet.as_ref().and_then(|s| {
            let found = page.reader.resolver().resolve(s, &row(STYLE_ROW))?;
            let text = page.reader.worked().text_of(s, &row(STYLE_ROW), &found)?.trim().to_owned();
            find_style(&text).map(|style| style.id.to_owned())
        });
        Ok(Settings {
            resize: number(RESIZE_ROW).map_or(Resize::Expand, Resize::from_number),
            margin: number(MARGIN_ROW).map_or(DEFAULT_MARGIN_INCHES * page.k, |m| if m.is_finite() { m.max(0.0) } else { 0.0 }),
            locked: number(LOCK_ROW).is_some_and(|n| n != 0.0),
            heading: number(HEADING_ROW).map_or(Side::Top, Side::from_number),
            band: if style.is_some() { BAND_INCHES * page.k } else { 0.0 },
            style,
        })
    }
}

/// Writes a `User` row of a container's settings on the shape at `at` of the page's list.
pub(super) fn put(doc: &mut Document, page: u32, at: usize, name: &str, cell: Cell) -> Result<(), Problem> {
    let index = u32::try_from(at).map_err(|_| Problem::failed("a shape's place is out of range"))?;
    let shape = shape_mut(doc, page, &[index]).ok_or_else(|| Problem::failed("a shape went away"))?;
    shape.counted_sheet().set_cell(&row(name), cell).map_err(|e| Problem::failed(e.to_string()))?;
    Ok(())
}

/// A whole-number cell.
pub(super) fn number_cell(n: f64) -> Cell {
    Cell::constant(Value::from_number(n).unwrap_or_else(|| Value::from_text("0")))
}

/// A length cell in drawing units.
pub(super) fn length_cell(n: f64) -> Cell {
    number_cell(n).with_unit("IN")
}

/// A text cell the way a file holds one: the string as the formula, and the text as the value.
pub(super) fn text_cell(text: &str) -> Cell {
    Cell::with_formula(&format!("\"{text}\""), Some(Value::from_text(text)))
}
