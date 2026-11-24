//! `_MARKER`: the reserved function that fills the alignment cells of a shape glued to guides
//! that hold no guide. Every one of the 1,142 saved cells holds `_MARKER(1)` with the value 1 and no
//! unit; a cell that reads it is in no sample.
//!
//! **The rule.** The argument comes back as a plain number (project rule: `_MARKER(3)` is 3; no unit, so a length handed in comes
//! back as its number of internal inches, ours). An argument that is an error comes back as the result (project rule: the registry does
//! it), and a call with no argument or more than one is `#VALUE!` (the registry).
//!
//! **Cost.** One argument worked out and a step for the call.

use crate::eval::{Evaluator, Site};
use crate::registry::{FnDef, eager};
use crate::value::Value;

type Slots<'a> = &'a [Option<Value>];

fn marker(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) => Value::num(q.stored),
        Err(e) => e,
    }
}

pub(crate) static DEFS: &[FnDef] = &[eager("_MARKER", 1, 1, marker)];
