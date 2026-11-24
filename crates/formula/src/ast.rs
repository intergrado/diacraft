//! The tree of a parsed formula.
//!
//! The tree is what the text says and no more. It holds names as written (the case of a cell name matters,
//! and a function name is matched without regard to case by whoever evaluates it), numbers as
//! written (`2.54 cm` is 2.54 and the unit `CM`, not the stored 1), and an operator as an operator: `a + b` is [`Expr::Binary`]
//! and `_ADD(a, b)` is a call, though both mean the same. Parentheses leave no node, only
//! the shape they give. Nothing here is evaluated, and no name is looked up.

use std::sync::Arc;

use diacraft_units::{Quantity, Unit};

/// A parsed formula expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A number, with its unit if it has one (`12`, `2.54 cm`, `3 IN^2`, `8FT 4IN`).
    Number(Number),
    /// `TRUE` or `FALSE`, in any case.
    Bool(bool),
    /// A string between double quotes. `""` inside one is a quote.
    Str(Arc<str>),
    /// A colour, `#` and six hex digits, red first.
    Color {
        /// Red.
        r: u8,
        /// Green.
        g: u8,
        /// Blue.
        b: u8,
    },
    /// A number followed by a currency code, `12.5USD` or `12.5 USD` (a blank may stand between, and the
    /// tree does not keep one). The parser reads only the 192 codes, in capitals ([`crate::CURRENCIES`]), so a parsed
    /// tree holds one of them; a tree built by hand may hold any text, and then its print does not parse back.
    Currency {
        /// The number, as written.
        value: f64,
        /// The three capital letters of the code.
        code: Arc<str>,
    },
    /// A GUID in braces with no quotes: `{`, eight, four, four, four and twelve hexadecimal digits with hyphens between, `}`
    /// It is read only as the one argument of `USE`, and it reads as the string it
    /// spells, so this holds that string with its braces, as written. There is no error literal: R55 says an error value
    /// only arises from evaluation, and `REF()` and `NA()` are the calls that make one.
    Guid(Arc<str>),
    /// A reference to a cell, `Width`, `Prop.Cost.Value`, `Sheet.12!PinX`. Names are not looked up.
    Ref(Box<CellRef>),
    /// A call, `IF(a, b, c)`, `_ADD(a, b)`, `Sheet.5!SHEETREF()`: any name followed by an argument list is a call, known or
    /// not.
    Call(Box<Call>),
    /// A unary sign: `-a` and `+a` (rank 2).
    Unary(UnaryOp, Box<Expr>),
    /// A binary operator (`+ - * / ^ &`, the comparisons).
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `a%` (rank 2). `40%` is `Percent(Number(40))`: the evaluator stores 0.4 with the unit `PER`.
    Percent(Box<Expr>),
}

impl Expr {
    /// The nodes of the tree as `MAX_NODES` and the budget of `parse_with_budget` count them: every expression one, every path
    /// segment one (a call's name is one, and so is a name in a qualifier), and every empty argument slot one. A `TRUE` or a
    /// `FALSE` is one. For a tree this crate parsed it is what the parse used; the walk keeps its own stack and does not recurse,
    /// so a tree of any depth is counted.
    pub fn node_count(&self) -> usize {
        let mut count = 0usize;
        let mut stack: Vec<&Expr> = vec![self];
        while let Some(e) = stack.pop() {
            count = count.saturating_add(1);
            match e {
                Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
                Expr::Binary(_, a, b) => {
                    stack.push(a);
                    stack.push(b);
                }
                Expr::Ref(r) => {
                    count = count.saturating_add(r.path.len());
                    count = count.saturating_add(qualifier_nodes(&r.qualifiers, &mut stack));
                }
                Expr::Call(c) => {
                    // The name is a segment.
                    count = count.saturating_add(1);
                    count = count.saturating_add(qualifier_nodes(&c.qualifiers, &mut stack));
                    count = count.saturating_add(slot_nodes(&c.args, &mut stack));
                }
                Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
            }
        }
        count
    }
}

/// The nodes of some qualifiers that are not expressions (names, and empty slots), and their argument expressions put on `stack`.
fn qualifier_nodes<'a>(qualifiers: &'a [Qualifier], stack: &mut Vec<&'a Expr>) -> usize {
    qualifiers
        .iter()
        .map(|q| match q {
            Qualifier::Path(segments) => segments.len(),
            Qualifier::Call { args, .. } => 1usize.saturating_add(slot_nodes(args, stack)),
        })
        .fold(0usize, usize::saturating_add)
}

/// The empty slots of an argument list, and its expressions put on `stack`.
fn slot_nodes<'a>(args: &'a [Option<Expr>], stack: &mut Vec<&'a Expr>) -> usize {
    let mut empty = 0usize;
    for arg in args {
        match arg {
            Some(e) => stack.push(e),
            None => empty = empty.saturating_add(1),
        }
    }
    empty
}

/// A number literal. It holds the numbers as written, never negative (a sign before a number is a [`UnaryOp`]) and always
/// finite. [`Number::quantity`] gives the stored value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    /// A number with one unit or none (`Unit::Unitless`): `12`, `2.54 cm`, `3 IN^2`. `power` is the whole power written
    /// straight after the unit, which belongs to the unit: `3 IN^2` is three square inches, where `(3 IN)^2`
    /// is the operator on a literal. A power may be negative and is not range-checked here (`#DIM!` is given elsewhere).
    Measured {
        /// The number as written.
        value: f64,
        /// The unit written after it.
        unit: Unit,
        /// The power of the unit, if one was written.
        power: Option<i32>,
    },
    /// `3[cm,D]`: a unit that follows the document's default for a measure. The number is in `unit`.
    Implicit {
        /// The number as written.
        value: f64,
        /// The unit written.
        unit: Unit,
        /// The measure whose default applies.
        measure: ImplicitMeasure,
    },
    /// `8FT 4IN`.
    FeetInches {
        /// Feet.
        feet: f64,
        /// Inches.
        inches: f64,
    },
    /// `10°15'30"`; minutes and seconds are 0 when not written.
    DegreesMinutesSeconds {
        /// Degrees.
        degrees: f64,
        /// Minutes.
        minutes: f64,
        /// Seconds.
        seconds: f64,
    },
    /// `2C4`: ciceros then didots.
    CiceroDidot {
        /// Ciceros.
        ciceros: f64,
        /// Didots.
        didots: f64,
    },
    /// `3P6`: picas then points.
    PicaPoints {
        /// Picas.
        picas: f64,
        /// Points.
        points: f64,
    },
}

/// The five measures an implicit unit names: `A` angle, `D` drawing, `E` duration, `P` page, `T` type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImplicitMeasure {
    /// `A`.
    Angle,
    /// `D`.
    Drawing,
    /// `E`.
    Duration,
    /// `P`.
    Page,
    /// `T`.
    Type,
}

impl ImplicitMeasure {
    /// The flag letter, in capitals.
    pub fn flag(self) -> char {
        match self {
            ImplicitMeasure::Angle => 'A',
            ImplicitMeasure::Drawing => 'D',
            ImplicitMeasure::Duration => 'E',
            ImplicitMeasure::Page => 'P',
            ImplicitMeasure::Type => 'T',
        }
    }

    /// The measure a flag letter names, in either case.
    pub fn from_flag(c: char) -> Option<ImplicitMeasure> {
        match c.to_ascii_uppercase() {
            'A' => Some(ImplicitMeasure::Angle),
            'D' => Some(ImplicitMeasure::Drawing),
            'E' => Some(ImplicitMeasure::Duration),
            'P' => Some(ImplicitMeasure::Page),
            'T' => Some(ImplicitMeasure::Type),
            _ => None,
        }
    }
}

impl Number {
    /// A plain number: no unit.
    pub const fn plain(value: f64) -> Number {
        Number::Measured { value, unit: Unit::Unitless, power: None }
    }

    /// The number with one unit and no power.
    pub const fn with_unit(value: f64, unit: Unit) -> Number {
        Number::Measured { value, unit, power: None }
    }

    /// The stored value and the unit it shows in, or `None` for a multidimensional literal (`3 IN^2`), whose dimension the
    /// evaluator tracks (and the note on `Quantity`).
    ///
    /// An implicit literal gives the number in the unit written: which unit it is shown in follows the document's default
    /// for [`ImplicitMeasure`], which the evaluator knows. A `DT` literal is stored as written, like `DL` (M1 Ruling
    /// 42): the units table has it so, and nothing here treats it apart.
    pub fn quantity(&self) -> Option<Quantity> {
        match *self {
            Number::Measured { power: Some(_), .. } => None,
            Number::Measured { value, unit, power: None } | Number::Implicit { value, unit, .. } => Some(Quantity::from_value(value, unit)),
            Number::FeetInches { feet, inches } => Some(Quantity::feet_inches(feet, inches)),
            Number::DegreesMinutesSeconds { degrees, minutes, seconds } => Some(Quantity::degrees_minutes_seconds(degrees, minutes, seconds)),
            Number::CiceroDidot { ciceros, didots } => Some(Quantity::cicero_didot(ciceros, didots)),
            Number::PicaPoints { picas, points } => Some(Quantity::pica_points(picas, points)),
        }
    }
}

/// The unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// `+a`, the call `_UPLUS`.
    Plus,
    /// `-a`, the call `_UMINUS`.
    Minus,
}

impl UnaryOp {
    /// The sign.
    pub fn symbol(self) -> &'static str {
        match self {
            UnaryOp::Plus => "+",
            UnaryOp::Minus => "-",
        }
    }

    /// The function spelling of the same operation.
    pub fn function_name(self) -> &'static str {
        match self {
            UnaryOp::Plus => "_UPLUS",
            UnaryOp::Minus => "_UMINUS",
        }
    }
}

/// The function spelling of `%`.
pub const PERCENT_FUNCTION: &str = "_PCT";

/// The binary operators, with the ranks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    /// `+`, `_ADD`.
    Add,
    /// `-`, `_SUB`.
    Sub,
    /// `*`, `_MUL`.
    Mul,
    /// `/`, `_DIV`.
    Div,
    /// `^`, `POW`.
    Pow,
    /// `&`, `_CAT`.
    Cat,
    /// `<`, `_FLT`: ordering with tolerance.
    Lt,
    /// `<=`, `_FLE`.
    Le,
    /// `>`, `_FGT`.
    Gt,
    /// `>=`, `_FGE`.
    Ge,
    /// `=`, `_FEQ`: equality with tolerance.
    Eq,
    /// `<>`, `_FNE`.
    Ne,
    /// `_LT_`, `LT`, `_ELT`: exact ordering.
    ExactLt,
    /// `_LE_`, `LE`, `_ELE`.
    ExactLe,
    /// `_GT_`, `GT`, `_EGT`.
    ExactGt,
    /// `_GE_`, `GE`, `_EGE`.
    ExactGe,
    /// `_EQ_`, `EQ`, `_EEQ`: exact equality.
    ExactEq,
    /// `_NE_`, `NE`, `_ENE`.
    ExactNe,
}

impl BinaryOp {
    /// The operator as printed: `+`, `<=`, `_GT_`. The word forms `GT` and the rest are read, never printed.
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Pow => "^",
            BinaryOp::Cat => "&",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Eq => "=",
            BinaryOp::Ne => "<>",
            BinaryOp::ExactLt => "_LT_",
            BinaryOp::ExactLe => "_LE_",
            BinaryOp::ExactGt => "_GT_",
            BinaryOp::ExactGe => "_GE_",
            BinaryOp::ExactEq => "_EQ_",
            BinaryOp::ExactNe => "_NE_",
        }
    }

    /// The function spelling of the same operation (`POW` for `^`).
    pub fn function_name(self) -> &'static str {
        match self {
            BinaryOp::Add => "_ADD",
            BinaryOp::Sub => "_SUB",
            BinaryOp::Mul => "_MUL",
            BinaryOp::Div => "_DIV",
            BinaryOp::Pow => "POW",
            BinaryOp::Cat => "_CAT",
            BinaryOp::Lt => "_FLT",
            BinaryOp::Le => "_FLE",
            BinaryOp::Gt => "_FGT",
            BinaryOp::Ge => "_FGE",
            BinaryOp::Eq => "_FEQ",
            BinaryOp::Ne => "_FNE",
            BinaryOp::ExactLt => "_ELT",
            BinaryOp::ExactLe => "_ELE",
            BinaryOp::ExactGt => "_EGT",
            BinaryOp::ExactGe => "_EGE",
            BinaryOp::ExactEq => "_EEQ",
            BinaryOp::ExactNe => "_ENE",
        }
    }

    /// The rank: 3 binds first, 8 last. Operators of one rank run left to right.
    pub fn rank(self) -> u8 {
        match self {
            BinaryOp::Pow => 3,
            BinaryOp::Mul | BinaryOp::Div => 4,
            BinaryOp::Add | BinaryOp::Sub => 5,
            BinaryOp::Cat => 6,
            BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
            | BinaryOp::ExactLt
            | BinaryOp::ExactLe
            | BinaryOp::ExactGt
            | BinaryOp::ExactGe => 7,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::ExactEq | BinaryOp::ExactNe => 8,
        }
    }
}

/// A call: `qualifier!NAME(args)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// The sheet qualifiers before the name, outermost first (`Sheet.5!SHEETREF()` has one). Empty for most calls.
    pub qualifiers: Vec<Qualifier>,
    /// The name as written. Match it without regard to case; a name no one knows is still a call.
    pub name: Arc<str>,
    /// The argument slots in order. An empty slot is `None` and keeps its place: `F(1,,3)` has three slots. `F()` has
    /// none.
    pub args: Vec<Option<Expr>>,
}

impl Call {
    /// Whether the name is `name`, in any letter case.
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

/// A reference to a cell, `qualifier!Section.Row.Cell`. The parts are kept as written; which sheet, section, row and cell
/// they name is for whoever has the document (`doc`, `calc`).
#[derive(Clone, Debug, PartialEq)]
pub struct CellRef {
    /// The sheet qualifiers before the cell, outermost first: `Pages[Site-2]!Sheet.9!EndY` has two.
    pub qualifiers: Vec<Qualifier>,
    /// The dotted path, at least one segment: `Prop.Cost.Label`, `Char.Size[2]`, `Scratch.B3`, `Geometry2.X4`, `Width`.
    pub path: Vec<Segment>,
}

/// One part of a sheet qualifier, before a `!`.
#[derive(Clone, Debug, PartialEq)]
pub enum Qualifier {
    /// A name: `Door`, `Door.7`, `Sheet.12`, `'Left-Door'`, `ThePage`, `TheDoc`, `Pages[Site-2]`, `Styles`.
    Path(Vec<Segment>),
    /// A call that returns a sheet: `LISTSHEETREF(2)!Width`.
    Call {
        /// The name as written.
        name: Arc<str>,
        /// The argument slots, as in [`Call::args`].
        args: Vec<Option<Expr>>,
    },
}

/// A name in a dotted path, with the `[index]` after it if there is one.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// The name as written, in its own case. A name in single quotes is given without them and with `''`
    /// read as `'`.
    pub name: Arc<str>,
    /// The `[...]` after the name: `Char.Size[2]`, `Pages[Site-2]`.
    pub index: Option<Index>,
}

impl Segment {
    /// A name with no index.
    pub fn new(name: &str) -> Segment {
        Segment { name: Arc::from(name), index: None }
    }
}

/// What a `[...]` holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Index {
    /// A whole number, `[2]`. For a row it is the row number of R40 (the row's `IX` plus one outside Geometry).
    Number(u32),
    /// Anything else, `[Site-2]`, or text in single quotes (`['2']` is the name `2`).
    Name(Arc<str>),
}
