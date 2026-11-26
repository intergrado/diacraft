//! The registry of functions: each name with the arguments it takes, and the code that works it out.
//!
//! A name matches in any letter case. A name not here (a reserved one such as `_SHAPEMIN`, a name a file's
//! extensions declare, or one no batch has landed yet) is [`crate::Value::Unsupported`]: the value is unknown and drawing uses the
//! saved one, as it does when a formula does not parse (is not written, so its names are not here; of
//! `_ELLIPSE_THETA`, `_ELLIPSE_ECC` and `_MARKER` are, and the other reserved names are not).
//!
//! **Eager and lazy.** An eager function has every argument worked out left to right before it runs, and gives back the first
//! failure among them unless it `sees_failures` (a function that is handed an error returns it). A lazy
//! one gets the call and works out only the arguments it needs: `IF` and its kin work out the branch they take, and the tests that
//! look for errors read the cell's error state through a bare reference.

use std::cmp::Ordering;
use std::sync::OnceLock;

use crate::eval::{Evaluator, Site};
use crate::value::Value;

/// An eager function: its arguments in slots, an empty one `None`.
pub(crate) type EagerFn = fn(&mut Evaluator<'_, '_>, &Site<'_>, &[Option<Value>]) -> Value;
/// A lazy function: the call itself.
pub(crate) type LazyFn = fn(&mut Evaluator<'_, '_>, &Site<'_>) -> Value;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Eager(EagerFn),
    Lazy(LazyFn),
}

/// One function.
#[derive(Clone, Copy)]
pub(crate) struct FnDef {
    /// The name in capitals.
    pub name: &'static str,
    /// The fewest and most argument slots; a call outside them is `#VALUE!` (the other batches').
    pub min: u8,
    pub max: u8,
    pub kind: Kind,
    /// Whether an eager function is given an argument that is an error or unsupported, instead of the call giving it back.
    pub sees_failures: bool,
}

/// No upper limit on the arguments.
pub(crate) const MANY: u8 = u8::MAX;

pub(crate) const fn eager(name: &'static str, min: u8, max: u8, f: EagerFn) -> FnDef {
    FnDef { name, min, max, kind: Kind::Eager(f), sees_failures: false }
}

/// An eager function that is handed an argument that is an error or unsupported, and decides what to do with it.
pub(crate) const fn eager_seeing(name: &'static str, min: u8, max: u8, f: EagerFn) -> FnDef {
    FnDef { name, min, max, kind: Kind::Eager(f), sees_failures: true }
}

pub(crate) const fn lazy(name: &'static str, min: u8, max: u8, f: LazyFn) -> FnDef {
    FnDef { name, min, max, kind: Kind::Lazy(f), sees_failures: false }
}

/// Every function of every batch landed, in no order.
fn all() -> Vec<&'static FnDef> {
    let mut v: Vec<&'static FnDef> = Vec::new();
    for batch in [
        crate::fn_ops::DEFS,
        crate::fn_math::DEFS,
        crate::fn_logic::DEFS,
        crate::fn_trig::DEFS,
        crate::fn_cells::DEFS,
        crate::fn_color::DEFS,
        crate::fn_date::DEFS,
        crate::fn_doc::DEFS,
        crate::fn_ellipse::DEFS,
        crate::fn_marker::DEFS,
        crate::fn_geom::DEFS,
        crate::fn_path::DEFS,
        crate::fn_text::DEFS,
    ] {
        v.extend(batch.iter());
    }
    v
}

/// Compares two names ignoring ASCII letter case, byte by byte.
fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    let mut x = a.bytes().map(|c| c.to_ascii_uppercase());
    let mut y = b.bytes().map(|c| c.to_ascii_uppercase());
    loop {
        match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) => match p.cmp(&q) {
                Ordering::Equal => {}
                other => return other,
            },
        }
    }
}

fn table() -> &'static [&'static FnDef] {
    static TABLE: OnceLock<Vec<&'static FnDef>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v = all();
        v.sort_by(|a, b| a.name.cmp(b.name));
        v
    })
}

/// The function called `name`, in any letter case.
pub(crate) fn lookup(name: &str) -> Option<&'static FnDef> {
    let t = table();
    t.binary_search_by(|d| cmp_ignore_case(d.name, name)).ok().and_then(|i| t.get(i)).copied()
}

/// The names of every function that is evaluated, in capitals and in order: for the report of functions implemented against specified.
pub fn function_names() -> Vec<&'static str> {
    table().iter().map(|d| d.name).collect()
}
