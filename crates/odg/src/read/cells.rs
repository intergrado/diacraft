//! Cells as the reader writes them: a *value* is a cell with `V` and no `F`, a
//! *formula* is `F` with its evaluated `V`, and `U` is the unit the value is shown in, which for a length is the unit of the attribute it
//! came from. `V` is always inches for lengths and radians for angles.
//!
//! **Formulas are parsed once each.** The reader writes a handful of formula texts (`Width*0.5`, `Height`, the 1-D cells), on every shape;
//! [`Forms`] parses each distinct text once and every cell that holds it shares the parse, so a page of 35,000 shapes holds a few dozen
//! parses, not 350,000. A text past [`MOST_FORMS`] distinct ones is parsed for its cell alone. A parse is charged by its nodes
//! ([`FORMULA_NODE_BYTES`] each) when it is made, so a formula costs what its tree holds and not what its text is long.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use diacraft_doc::{Cell, FormulaSlot, Value};
use diacraft_formula::Formula;

/// The longest text of a cell that is compared as it is, or written into a key as it is. A cell's text longer than this is a style's text (a
/// font's family, a bullet's characters), made once for each distinct text and shared by every cell that shows it (`StyleTexts::value`), so
/// two such texts are the same exactly when they are the same value, and are told apart by where they are held and not by reading them. The
/// texts the reader makes of numbers, colours and units are no longer than 25 characters, so never reach it.
pub(crate) const SHORT_TEXT: usize = 64;

/// The longest formula text kept for sharing: the reader's own formulas (`Width*0.5`, a path's shares, the 1-D cells) are short, and a longer
/// one (an equation, an arc's ray formula) is never written twice, so it is parsed for its cell alone and no copy of it is kept here.
const MOST_FORM_BYTES: usize = 256;

/// Distinct formula texts kept for sharing. The reader's own formulas number a few dozen; shares (`Width*0.25`) add one per distinct share.
const MOST_FORMS: usize = 4096;

/// The bytes of the model one node of a parsed formula stands for. The formula crate measured a tree at 72 bytes a node for the arguments of
/// a call and 128 for a name with a qualifier (`MAX_NODES` in its docs); the larger is charged for every node, so that a tree is never
/// charged less than it holds. A tree of 25 bytes a node (a sum of numbers) is over-charged, and one of names is charged what it costs.
pub(crate) const FORMULA_NODE_BYTES: usize = 128;

/// The formulas parsed so far, by text, and what the trees parsed since the last charge cost.
///
/// **Every tree is charged.** A formula's text and its parse are what a cell holds, and the parse is several times the text (a sum of
/// numbers is 25 times, a name with a qualifier 40 times), so a charge by the text's length undercharged it by a factor of eight (the
/// A 3 KB file of repeated closes of one long formula aborted the process). A tree is charged by its node count
/// ([`FORMULA_NODE_BYTES`] each) and the text it keeps, when it is parsed; a text shared with an earlier cell is parsed once and charged
/// once. The charge is taken with the next items the reader takes (`Reader::take_items`, through [`Forms::settle`]): the cell is stored in
/// the model after the row that holds it is charged, so a tree the document cannot afford is dropped before it is stored.
pub(crate) struct Forms {
    parsed: HashMap<String, Arc<Formula>>,
    /// Bytes and nodes of the trees parsed since the last [`Forms::settle`].
    unpaid_bytes: usize,
    unpaid_nodes: u64,
    /// The nodes the document may still parse: a tree is not built past it (`parse_with_budget`), however the charges stand.
    nodes_left: usize,
    /// A tree was refused for the document's budget (and not for the formula crate's own limit on one formula): the next charge is refused.
    over_budget: bool,
}

impl Default for Forms {
    fn default() -> Forms {
        Forms { parsed: HashMap::new(), unpaid_bytes: 0, unpaid_nodes: 0, nodes_left: usize::MAX, over_budget: false }
    }
}

impl Forms {
    /// A set of formulas that parses at most `nodes` nodes in all.
    pub(crate) fn with_budget(nodes: usize) -> Forms {
        Forms { nodes_left: nodes, ..Forms::default() }
    }

    /// The parse of `source`, shared with every earlier cell that holds the same text.
    fn formula(&mut self, source: &str) -> Arc<Formula> {
        if let Some(found) = self.parsed.get(source) {
            return Arc::clone(found);
        }
        let made = Arc::new(diacraft_formula::parse_with_budget(source, self.nodes_left));
        if self.nodes_left < diacraft_formula::MAX_NODES && matches!(&made.ast, Err(e) if e.kind == diacraft_formula::ParseErrorKind::TooManyNodes) {
            self.over_budget = true;
        }
        let nodes = made.node_count();
        self.nodes_left = self.nodes_left.saturating_sub(nodes);
        let kept = self.parsed.len() < MOST_FORMS && source.len() <= MOST_FORM_BYTES;
        // The text is held by the formula, and again as the key of a shared one.
        // As text, at nine eighths of its length.
        let text = super::cost::text(if kept { source.len().saturating_mul(2) } else { source.len() });
        self.unpaid_bytes = self.unpaid_bytes.saturating_add(text).saturating_add(nodes.saturating_mul(FORMULA_NODE_BYTES));
        self.unpaid_nodes = self.unpaid_nodes.saturating_add(nodes as u64);
        if kept {
            self.parsed.insert(source.to_owned(), Arc::clone(&made));
        }
        made
    }

    /// What the trees parsed since the last call cost in bytes of the model, and their nodes: taken, so that each is charged once.
    pub(crate) fn settle(&mut self) -> (usize, u64) {
        (std::mem::take(&mut self.unpaid_bytes), std::mem::take(&mut self.unpaid_nodes))
    }

    /// Whether a tree was refused for the document's budget of nodes: the cell that holds it has the text and no tree, and the reader
    /// refuses the document at its next charge, as it refuses one past the items (`Reader::take_items`).
    pub(crate) fn over_budget(&self) -> bool {
        self.over_budget
    }

    /// A formula cell: `source`, evaluated to `value`, shown in `unit`.
    pub(crate) fn cell(&mut self, source: &str, value: f64, unit: Option<&str>) -> Cell {
        Cell { formula: FormulaSlot::Local(self.formula(source)), value: Some(number(value)), unit: unit.map(unit_arc), ..Cell::default() }
    }

    /// A formula cell whose evaluated value is text (a field's shown text).
    pub(crate) fn text_cell(&mut self, source: &str, shown: &str) -> Cell {
        Cell { formula: FormulaSlot::Local(self.formula(source)), value: Some(Value::from_text(shown)), ..Cell::default() }
    }
}

/// A number as a value's text: the shortest text that reads back as it. A number that is not finite is never written; the reader checks
/// what it computes before it gets here, and 0 stands in.
pub(crate) fn number(n: f64) -> Value {
    // The values most cells hold are one text each, shared: a page of shapes holds thousands of 0s and 1s.
    static SHARED: OnceLock<[Value; 4]> = OnceLock::new();
    let shared = SHARED.get_or_init(|| ["0", "1", "-1", "0.5"].map(Value::from_text));
    let n = if n.is_finite() { n } else { 0.0 };
    let at = if n == 0.0 && n.is_sign_positive() {
        Some(0)
    } else if n == 1.0 {
        Some(1)
    } else if n == -1.0 {
        Some(2)
    } else if n == 0.5 {
        Some(3)
    } else {
        None
    };
    match at.and_then(|i| shared.get(i)) {
        Some(v) => v.clone(),
        None => Value::from_number(n).unwrap_or_else(|| Value::from_text("0")),
    }
}

/// A unit's text as a cell holds it, shared for the units the reader writes.
fn unit_arc(word: &str) -> Arc<str> {
    static SHARED: OnceLock<[Arc<str>; 6]> = OnceLock::new();
    let shared = SHARED.get_or_init(|| ["CM", "MM", "IN", "PT", "P", "DEG"].map(Arc::from));
    shared.iter().find(|u| u.as_ref() == word).cloned().unwrap_or_else(|| Arc::from(word))
}

/// A value cell holding a number, with no unit.
pub(crate) fn plain(n: f64) -> Cell {
    Cell::constant(number(n))
}

/// A value cell holding a length, `inches`, shown in `unit`.
pub(crate) fn length(inches: f64, unit: Option<&str>) -> Cell {
    Cell { value: Some(number(inches)), unit: unit.map(unit_arc), ..Cell::default() }
}

/// A value cell holding text exactly (a colour `#RRGGBB`, a name).
pub(crate) fn text(value: &str) -> Cell {
    Cell::constant(Value::from_text(value))
}

/// A value cell holding an angle in radians, shown in degrees.
pub(crate) fn angle(radians: f64) -> Cell {
    length(radians, Some("DEG"))
}

/// A share of `of` (`Width` or `Height`) as DiaCraft's basic shapes write one: the constant 0 for 0,
/// the bare name for 1, and `Width*<k>` otherwise, `k` rounded to nine places, with its value at `size`.
pub(crate) fn share(forms: &mut Forms, of: &str, k: f64, size: f64, unit: Option<&str>) -> Cell {
    share_exact(forms, of, round9(k), size, unit)
}

/// [`share`] with `k` as it is, every digit of it: the cell of a handle's position, which the formula of the modifier it drives reads back (`X/Width*21600`), so that a
/// share cut to nine places would make the modifier work out to a number that is not the file's.
pub(crate) fn share_exact(forms: &mut Forms, of: &str, k: f64, size: f64, unit: Option<&str>) -> Cell {
    if k == 0.0 {
        length(0.0, unit)
    } else if k == 1.0 {
        forms.cell(of, size, unit)
    } else if k == -1.0 {
        forms.cell(&format!("-{of}"), -size, unit)
    } else if k < 0.0 {
        forms.cell(&format!("-{of}*{}", number(-k).text()), k * size, unit)
    } else {
        forms.cell(&format!("{of}*{}", number(k).text()), k * size, unit)
    }
}

/// The formula text of the cell [`share`] gives for `k` (`None` for 0, the constant): what the reader makes of a share of `of` in a basic shape's outline, so that a writer can tell a row the
/// reader gives back with its formula from one whose formula it does not (`Width*1` reads back as `Width`, `Width*0` as the constant 0).
pub(crate) fn share_text(of: &str, k: f64) -> Option<String> {
    let k = round9(k);
    if k == 0.0 {
        None
    } else if k == 1.0 {
        Some(of.to_owned())
    } else if k == -1.0 {
        Some(format!("-{of}"))
    } else if k < 0.0 {
        Some(format!("-{of}*{}", number(-k).text()))
    } else {
        Some(format!("{of}*{}", number(k).text()))
    }
}

/// `n` rounded to nine decimal places (the precision DiaCraft writes shares in).
pub(crate) fn round9(n: f64) -> f64 {
    let r = (n * 1.0e9).round() / 1.0e9;
    if r.is_finite() { r } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text [`share_text`] gives is the formula of the cell [`share`] makes, for every kind of share.
    #[test]
    fn the_text_of_a_share_is_the_formula_of_its_cell() {
        let mut forms = Forms::default();
        for k in [0.0, 1.0, -1.0, 0.5, 0.25, -0.75, 0.123_456_789_4, 1e-10, 2.0] {
            assert_eq!(share_text("Width", k).as_deref(), share(&mut forms, "Width", k, 2.0, None).formula.text(), "{k}");
        }
    }

    #[test]
    fn shares_take_the_basic_shapes_form() {
        let mut forms = Forms::default();
        let zero = share(&mut forms, "Width", 0.0, 2.0, Some("IN"));
        assert!(matches!(zero.formula, FormulaSlot::Absent));
        assert_eq!(zero.value.as_ref().map(Value::text), Some("0"));
        let whole = share(&mut forms, "Height", 1.0, 2.0, None);
        assert_eq!(whole.formula.text(), Some("Height"));
        assert_eq!(whole.value.as_ref().map(Value::text), Some("2"));
        let half = share(&mut forms, "Width", 0.5, 3.0, None);
        assert_eq!(half.formula.text(), Some("Width*0.5"));
        assert_eq!(half.value.as_ref().and_then(Value::number), Some(1.5));
        let less = share(&mut forms, "Width", -0.25, 4.0, None);
        assert_eq!(less.formula.text(), Some("-Width*0.25"));
        assert_eq!(less.value.as_ref().and_then(Value::number), Some(-1.0));
        let minus = share(&mut forms, "Width", -1.0, 4.0, None);
        assert_eq!(minus.formula.text(), Some("-Width"));
        assert_eq!(minus.value.as_ref().and_then(Value::number), Some(-4.0));
        assert_eq!(share(&mut forms, "Width", 0.123_456_789_4, 1.0, None).formula.text(), Some("Width*0.123456789"));
        // One parse for one text.
        let (a, b) = (forms.cell("Width*0.5", 1.0, None), forms.cell("Width*0.5", 2.0, None));
        match (a.formula, b.formula) {
            (FormulaSlot::Local(x), FormulaSlot::Local(y)) => assert!(Arc::ptr_eq(&x, &y)),
            other => panic!("{other:?}"),
        }
    }

    /// A tree is charged by its nodes and its text when it is parsed, and a text shared with an earlier cell is charged once.
    #[test]
    fn a_parsed_formula_is_charged_by_its_nodes_and_its_text() {
        // Text is charged at nine eighths of its length, rounded up (`cost::text`).
        use crate::read::cost::text;
        let nodes = |text: &str| diacraft_formula::parse(text).node_count();
        let mut forms = Forms::default();
        assert_eq!(forms.settle(), (0, 0));
        // A short text is kept for sharing: its text twice, and each node at the formula crate's bytes.
        forms.cell("Width*0.5", 1.0, None);
        let n = nodes("Width*0.5");
        assert!(n >= 3);
        assert_eq!(forms.settle(), (text(2 * 9) + n * FORMULA_NODE_BYTES, n as u64));
        // Nothing more is taken for it, and nothing is taken twice.
        forms.cell("Width*0.5", 2.0, None);
        assert_eq!(forms.settle(), (0, 0));
        // A text past the sharing limit is parsed for each cell, and charged for each, its text once.
        let long = format!("1{}", "+1".repeat(200));
        assert_eq!(long.len(), 401);
        forms.cell(&long, 1.0, None);
        let first = forms.settle();
        forms.cell(&long, 1.0, None);
        assert_eq!(forms.settle(), first);
        assert_eq!(first, (text(401) + 401 * FORMULA_NODE_BYTES, 401));
        // A text that does not parse costs its text alone.
        forms.cell("1+", 1.0, None);
        assert_eq!(forms.settle(), (text(4), 0));
    }

    /// The texts kept for sharing are at most `MOST_FORMS`: the one past it is parsed for its cell alone and its text is charged once.
    #[test]
    fn the_texts_kept_for_sharing_are_charged_twice_up_to_the_cap() {
        // Text is charged at nine eighths of its length, rounded up (`cost::text`).
        use crate::read::cost::text;
        let mut forms = Forms::default();
        for i in 0..MOST_FORMS - 1 {
            forms.cell(&format!("{i}+1"), 1.0, None);
        }
        forms.settle();
        // The last one that is kept: its text twice.
        forms.cell("a+1", 1.0, None);
        let n = diacraft_formula::parse("a+1").node_count();
        assert_eq!(forms.settle(), (text(2 * 3) + n * FORMULA_NODE_BYTES, n as u64));
        // The next is not kept: its text once, and it is parsed again for the next cell that holds it.
        forms.cell("b+1", 1.0, None);
        assert_eq!(forms.settle(), (text(3) + n * FORMULA_NODE_BYTES, n as u64));
        forms.cell("b+1", 1.0, None);
        assert_eq!(forms.settle(), (text(3) + n * FORMULA_NODE_BYTES, n as u64));
    }

    /// A document's budget of nodes: a tree is not built past it, and a formula past it is a formula that did not parse.
    #[test]
    fn a_tree_is_not_built_past_the_budget_of_nodes() {
        let mut forms = Forms::with_budget(10);
        let parsed = |cell: &Cell| matches!(&cell.formula, FormulaSlot::Local(f) if f.is_parsed());
        assert!(parsed(&forms.cell("1+1", 1.0, None)), "3 nodes of 10, and 7 are left");
        assert!(!parsed(&forms.cell("1+1+1+1+1", 1.0, None)), "9 nodes are past the 7 that are left");
        assert!(parsed(&forms.cell("1+1+1+1", 1.0, None)), "7 nodes are the 7 that are left");
        let refused = forms.cell("1-1", 1.0, None);
        assert!(!parsed(&refused), "no room is left");
        // The source of a refused formula is kept as it was given.
        assert_eq!(refused.formula.text(), Some("1-1"));
        assert_eq!(forms.settle().1, 3 + 7);
        assert!(forms.over_budget());
        // A formula that is too large for the formula crate's own limit is not the budget's refusal.
        let mut roomy = Forms::default();
        let too_many = (0..17).fold("1".to_owned(), |s, _| format!("({s}+{s})"));
        let cell = roomy.cell(&too_many, 1.0, None);
        assert!(
            matches!(&cell.formula, FormulaSlot::Local(f) if matches!(&f.ast, Err(e) if e.kind == diacraft_formula::ParseErrorKind::TooManyNodes))
        );
        assert!(!roomy.over_budget());
        // A budget as large as the formula crate's own limit is that limit: its refusal is not the budget's.
        let mut equal = Forms::with_budget(diacraft_formula::MAX_NODES);
        equal.cell(&too_many, 1.0, None);
        assert!(!equal.over_budget());
        let mut under = Forms::with_budget(diacraft_formula::MAX_NODES - 1);
        under.cell(&too_many, 1.0, None);
        assert!(under.over_budget());
        // Nor is a budget that was not reached.
        let mut spare = Forms::with_budget(10);
        spare.cell("1+1", 1.0, None);
        assert!(!spare.over_budget());
    }

    /// A formula text of up to 256 bytes is parsed once and shared; a longer one is parsed for its cell alone and no copy is kept.
    #[test]
    fn a_long_formula_text_is_not_kept_for_sharing() {
        let mut forms = Forms::default();
        let same = |forms: &mut Forms, text: &str| match (forms.cell(text, 1.0, None).formula, forms.cell(text, 1.0, None).formula) {
            (FormulaSlot::Local(a), FormulaSlot::Local(b)) => Arc::ptr_eq(&a, &b),
            other => panic!("{other:?}"),
        };
        let at_the_limit = format!("10{}", "+1".repeat(127));
        let past_it = format!("100{}", "+1".repeat(127));
        assert_eq!((at_the_limit.len(), past_it.len()), (MOST_FORM_BYTES, MOST_FORM_BYTES + 1));
        assert!(same(&mut forms, &at_the_limit));
        assert!(!same(&mut forms, &past_it));
        assert_eq!(forms.parsed.len(), 1, "only the short one is kept");
    }
}
