//! What reading a sheet says about the file besides the sheet: a warning, and a formula that did not parse.
//!
//! The codec does not own a report: each format has its own (`ReadReport` of the VSDX reader), and the codec says what it saw through the
//! caller's [`Diagnostics`]. A message never holds text from the file raw: it holds words, numbers and part names (escaped).

use std::collections::BTreeMap;

/// Where the codec says what was odd.
pub trait Diagnostics {
    /// A warning of `class`, a fixed word: counted always, worded while there is room. `message` is built only when it is kept.
    fn warn(&mut self, class: &'static str, message: impl FnOnce() -> String);

    /// A formula that did not parse, by its error's class word (`ParseErrorKind::name`).
    fn formula_error(&mut self, class: &'static str);
}

/// A [`Diagnostics`] that keeps the first `cap` messages and counts every warning and every formula error by class.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Warnings {
    /// The first `cap` warnings, in words, in the order they were made.
    pub messages: Vec<String>,
    /// Every warning by class word, exactly counted.
    pub counts: BTreeMap<&'static str, u64>,
    /// Formulas that did not parse, by class word.
    pub formula_errors: BTreeMap<&'static str, u64>,
    cap: usize,
}

impl Warnings {
    /// A collector that words at most `cap` warnings; the rest are only counted.
    #[must_use]
    pub fn new(cap: usize) -> Warnings {
        Warnings { cap, ..Warnings::default() }
    }

    /// How many warnings of this class there were.
    #[must_use]
    pub fn count(&self, class: &str) -> u64 {
        self.counts.get(class).copied().unwrap_or(0)
    }
}

impl Diagnostics for Warnings {
    fn warn(&mut self, class: &'static str, message: impl FnOnce() -> String) {
        let n = self.counts.entry(class).or_insert(0);
        *n = n.saturating_add(1);
        if self.messages.len() < self.cap {
            self.messages.push(message());
        }
    }

    fn formula_error(&mut self, class: &'static str) {
        let n = self.formula_errors.entry(class).or_insert(0);
        *n = n.saturating_add(1);
    }
}
