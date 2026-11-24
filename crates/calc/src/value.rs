//! What a formula evaluates to.
//!
//! A value is one of the classes: a string, a number (with its unit, dimension and a date among
//! the units), a Boolean, a currency, a colour, a point, a constructor of a path, or an error. Two more are the evaluator's own: a
//! sheet (what `LISTSHEETREF()` and its kin give), which no function but a qualifier takes, and
//! [`Value::Unsupported`], which a function the evaluator does not have gives, and which anything built on it shares, so that drawing
//! uses the saved value as it does when a formula does not parse.

use std::sync::Arc;

use diacraft_doc::SheetRef;
use diacraft_formula::Currency;
use diacraft_units::{Quantity, Unit};

/// An error value. [`CellError::Name`] is outside MS-VSDX's list; files hold it in `E` (17 cells),
/// and a reader keeps it, but nothing here makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CellError {
    /// `#DIV/0!`.
    DivZero,
    /// `#VALUE!`.
    Value,
    /// `#REF!`.
    Ref,
    /// `#NUM!`.
    Num,
    /// `#N/A`.
    Na,
    /// `#DIM!`.
    Dim,
    /// `#NAME?`.
    Name,
}

impl CellError {
    /// Every error, once.
    pub const ALL: [CellError; 7] =
        [CellError::DivZero, CellError::Value, CellError::Ref, CellError::Num, CellError::Na, CellError::Dim, CellError::Name];

    /// The spelling `E` holds.
    pub fn text(self) -> &'static str {
        match self {
            CellError::DivZero => "#DIV/0!",
            CellError::Value => "#VALUE!",
            CellError::Ref => "#REF!",
            CellError::Num => "#NUM!",
            CellError::Na => "#N/A",
            CellError::Dim => "#DIM!",
            CellError::Name => "#NAME?",
        }
    }

    /// A short class name for reports: no formula text and no value, only the kind of failure.
    pub fn class(self) -> &'static str {
        match self {
            CellError::DivZero => "div0",
            CellError::Value => "value",
            CellError::Ref => "ref",
            CellError::Num => "num",
            CellError::Na => "na",
            CellError::Dim => "dim",
            CellError::Name => "name",
        }
    }

    /// The error a text names: the spelling of R55, and the others of D1 (`#N/A!`, `#NA!`, `#NUM`, `#DIV/0`). `None` for any other text.
    pub fn from_text(text: &str) -> Option<CellError> {
        let t = text.trim();
        Some(match t {
            "#DIV/0!" | "#DIV/0" => CellError::DivZero,
            "#VALUE!" | "#VALUE" => CellError::Value,
            "#REF!" | "#REF" => CellError::Ref,
            "#NUM!" | "#NUM" => CellError::Num,
            "#N/A" | "#N/A!" | "#NA!" | "#NA" => CellError::Na,
            "#DIM!" | "#DIM" => CellError::Dim,
            "#NAME?" | "#NAME" => CellError::Name,
            _ => return None,
        })
    }
}

/// A colour, red first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// `#rrggbb`, in lower case, as a file prints a colour a function made.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// `#RRGGBB`, either case.
    pub fn from_hex(text: &str) -> Option<Rgb> {
        let digits = text.trim().strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |at: usize| digits.get(at..at + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        Some(Rgb { r: byte(0)?, g: byte(2)?, b: byte(4)? })
    }
}

/// A point (`PNT`): two numbers with their units, and the sheet whose frame they are in. A point made
/// from another shape's cells is in that shape's frame whatever the cells mean, so the tag comes from the references; with
/// none the point is in the sheet's own frame (`frame` is `None`).
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub x: Quantity,
    pub y: Quantity,
    pub frame: Option<SheetRef>,
    /// Set when the references of the point name more than one sheet: the numbers are kept, and a
    /// function that carries the point between frames gives `#VALUE!`.
    pub mixed: bool,
}

/// What a `NURBS` or `POLYLINE` call makes: the numbers, in the order given.
#[derive(Clone, Debug, PartialEq)]
pub struct Constructor {
    pub kind: ConstructorKind,
    pub numbers: Vec<f64>,
}

/// Which constructor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructorKind {
    Nurbs,
    Polyline,
}

/// A value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A number with its unit and dimension; a date is a number whose unit is [`Unit::Date`].
    Number(Quantity),
    /// A currency: the amount and the entry of it is in, or `None` for the undefined currency (ID -1).
    Currency { amount: f64, currency: Option<&'static Currency> },
    /// A string.
    Text(Arc<str>),
    /// A Boolean.
    Bool(bool),
    /// A colour.
    Color(Rgb),
    /// A point.
    Point(Box<Point>),
    /// The result of `NURBS` or `POLYLINE`.
    Constructor(Box<Constructor>),
    /// A sheet.
    Sheet(Box<SheetRef>),
    /// An error value.
    Error(CellError),
    /// The name of a function this evaluator does not have: the value is unknown, and the saved one stands.
    Unsupported(Arc<str>),
}

impl Value {
    /// A plain number, with no unit.
    pub fn num(x: f64) -> Value {
        Value::Number(Quantity::new(x, Unit::Unitless))
    }

    /// A number as stored, in a unit.
    pub fn stored(stored: f64, unit: Unit) -> Value {
        Value::Number(Quantity::new(stored, unit))
    }

    /// A date, given its serial.
    pub fn date(serial: f64) -> Value {
        Value::Number(Quantity::new(serial, Unit::Date))
    }

    /// A string.
    pub fn text(s: &str) -> Value {
        Value::Text(Arc::from(s))
    }

    /// An unsupported function.
    pub fn unsupported(name: &str) -> Value {
        Value::Unsupported(Arc::from(name))
    }

    /// The error, if this is one.
    pub fn error(&self) -> Option<CellError> {
        match self {
            Value::Error(e) => Some(*e),
            _ => None,
        }
    }

    /// Whether this is an error or an unsupported function: what an argument may be that a function must not work on, and which it
    /// hands back.
    pub fn is_failure(&self) -> bool {
        matches!(self, Value::Error(_) | Value::Unsupported(_))
    }

    /// The number, if this is one.
    pub fn quantity(&self) -> Option<Quantity> {
        match self {
            Value::Number(q) => Some(*q),
            _ => None,
        }
    }

    /// The bytes the value holds on the heap beyond itself, as accounted: its text, or its numbers.
    pub fn heap_bytes(&self) -> u64 {
        match self {
            Value::Text(t) => t.len() as u64,
            Value::Unsupported(n) => n.len() as u64,
            Value::Constructor(c) => 8 * c.numbers.len() as u64,
            _ => 0,
        }
    }

    /// A short name of the kind, for messages and reports.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Number(q) if q.unit == Unit::Date => "date",
            Value::Number(_) => "number",
            Value::Currency { .. } => "currency",
            Value::Text(_) => "text",
            Value::Bool(_) => "bool",
            Value::Color(_) => "color",
            Value::Point(_) => "point",
            Value::Constructor(_) => "constructor",
            Value::Sheet(_) => "sheet",
            Value::Error(_) => "error",
            Value::Unsupported(_) => "unsupported",
        }
    }
}
