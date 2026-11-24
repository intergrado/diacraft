//! `state.xml`: the identifier ledger and the content stamp (the public spec's 2.7).
//!
//! It holds the bookkeeping of the program that wrote the file, not part of the drawing's content.
//!
//! * **The ledger** says, for each page, the highest shape `id` ever given to a shape of it, so that a program that gives new shapes their `id` gives each one above
//!   the mark and the `id` of a deleted shape is not given again: formulas, connections, held data and the data carried in other files name shapes by `id`, and a
//!   reused `id` would quietly point them at a new shape. **A ledger can raise a mark and never lower one**: a mark below the highest shape `id` on its page reads as
//!   that `id` (so a hostile ledger of 3 on a page that holds shape 9 reads as 9); when the ledger has more than one row for a page the highest is taken; a row for a
//!   page the drawing does not have is ignored. A writer writes one row for each page with a mark above 0, and neither a second row for a page nor a row for a page
//!   the drawing lacks. A mark is a `u32`, and `4294967295` is a mark like any other: **the first `id` above it does not exist**, and it is the engine that says so.
//! * **The content stamp** is the stamp of the model as it was when it was read from a VSDX or OpenDocument file (3.17), so that a save to a foreign format can tell
//!   whether the drawing was edited since. Only the program that made it can use it; a file with none is a drawing that never came from a foreign file.
//!
//! A reader ignores the elements and attributes of the part that it does not know; a part with none is an empty state: every mark is 0 and the stamp is unknown.
//!
//! **What a hostile ledger costs** (the report measured 186 ms for 200,001 rows against 2.9 ms for a clean open). The part is a tree, read whole at the
//! items of one tree (2 million: the root and its declaration are two, a row is three), so **a ledger has 666,666 rows at the most**; each row is walked once, and a mark is kept only
//! for a page the drawing has (at most 1,000), so what the read holds of it is one mark for a page and no more. **The cost is linear in the bytes of the part and bounded by that
//! tree**: counted by `tests/it/ledger_cost.rs` (an event of the parser for each row, and nothing that grows faster) and timed beside plain pages of the same size (the ledger of 200,001
//! rows, 8.4 MB, is read in about 0.1 s, plain pages of that size in 0.38 s: a row costs less per byte than a shape does). A ledger past the tree is refused as `xml_too_complex`, at the item
//! that passes it (the parse stops there).

use std::collections::{BTreeMap, HashSet};

use diacraft_container::xml::{El, Node, to_bytes};
use diacraft_doc::{Document, Shape};
use diacraft_sheetxml::model::{Stamp, URN};

use crate::error::NativeError;
use crate::xmlns::{Declarations, attr, is_ours};

const PREFIX: &str = "diacraft";

/// The identifier ledger: for each page `id`, the highest shape `id` ever given to a shape of that page (a `u32`; a page with no mark is not in it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger(pub BTreeMap<u32, u32>);

/// The bookkeeping `state.xml` holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The ledger (2.7).
    pub ledger: Ledger,
    /// The content stamp of the model when the drawing was read from a foreign file, if it was (3.17).
    pub content_stamp: Option<Stamp>,
}

/// A whole number in decimal digits that fits a `u32`.
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 10 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The highest shape `id` on each page of `doc`, nested shapes included: a page with no shape is not in the map.
///
/// A walk with a stack of its own, so that a group nested to the model's 200 levels costs no recursion; it visits each shape once.
pub(crate) fn highest_ids(doc: &Document) -> BTreeMap<u32, u32> {
    let mut out: BTreeMap<u32, u32> = BTreeMap::new();
    for page in doc.pages.iter() {
        let mut highest: Option<u32> = None;
        let mut stack: Vec<&Shape> = page.shapes.iter().map(|s| &**s).collect();
        while let Some(shape) = stack.pop() {
            highest = Some(highest.map_or(shape.id, |h| h.max(shape.id)));
            stack.extend(shape.children.iter().map(|s| &**s));
        }
        if let Some(h) = highest {
            let slot = out.entry(page.id).or_insert(h);
            *slot = (*slot).max(h);
        }
    }
    out
}

/// Reads `state.xml` from its tree for a drawing whose pages have the `id`s in `pages`, and raises each mark to the highest shape `id` of its page (`highest`).
/// The elements and attributes it does not know are ignored; a `content-stamp` that is not a stamp, and a `ledger` row without a `page` or a `mark`, or with a
/// number that is not one, make the part damaged.
pub(crate) fn parse(root: &El, pages: &HashSet<u32>, highest: &BTreeMap<u32, u32>) -> Result<State, NativeError> {
    let bad = || NativeError::damaged("state_malformed");
    let own = Declarations::of(root).ok_or_else(bad)?;
    if root.local() != "state" || !is_ours(root, &own, None) {
        return Err(bad());
    }
    let content_stamp = match attr(root, "content-stamp") {
        None => None,
        Some(text) => Some(Stamp::parse(text).ok_or_else(bad)?),
    };
    let mut marks: BTreeMap<u32, u32> = BTreeMap::new();
    for child in root.children.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None }) {
        let theirs = Declarations::of(child).ok_or_else(bad)?;
        if child.local() != "ledger" || !is_ours(child, &theirs, Some(&own)) {
            continue;
        }
        let page = attr(child, "page").and_then(number).ok_or_else(bad)?;
        let mark = attr(child, "mark").and_then(number).ok_or_else(bad)?;
        // A row for a page the drawing does not have is ignored; the highest of the rows for a page is its mark.
        if pages.contains(&page) {
            let slot = marks.entry(page).or_insert(mark);
            *slot = (*slot).max(mark);
        }
    }
    // A ledger raises a mark and never lowers one: no mark is below the highest shape `id` of its page.
    for (page, shape) in highest {
        let slot = marks.entry(*page).or_insert(*shape);
        *slot = (*slot).max(*shape);
    }
    marks.retain(|_, mark| *mark > 0);
    Ok(State { ledger: Ledger(marks), content_stamp })
}

/// The bytes of `state.xml` for a drawing with pages `pages` whose shapes have the highest `id`s `highest`: the content stamp when there is one, and a `ledger` row
/// for each page that has a mark above 0, in page order, the mark being the state's raised to the highest shape `id` of its page (so the file never lowers one).
/// A row for a page the drawing lacks is not written, and a page appears once.
pub(crate) fn write(state: &State, pages: &[u32], highest: &BTreeMap<u32, u32>) -> Result<Vec<u8>, NativeError> {
    let mut root = El { name: format!("{PREFIX}:state"), attrs: vec![(format!("xmlns:{PREFIX}"), URN.to_owned())], children: Vec::new() };
    if let Some(stamp) = state.content_stamp {
        root.attrs.push(("content-stamp".to_owned(), stamp.text()));
    }
    let mut written: BTreeMap<u32, u32> = BTreeMap::new();
    for page in pages {
        let mark = state.ledger.0.get(page).copied().unwrap_or(0).max(highest.get(page).copied().unwrap_or(0));
        if mark > 0 {
            written.insert(*page, mark);
        }
    }
    for (page, mark) in written {
        let ledger = El {
            name: format!("{PREFIX}:ledger"),
            attrs: vec![("page".to_owned(), page.to_string()), ("mark".to_owned(), mark.to_string())],
            children: Vec::new(),
        };
        root.children.push(Node::El(ledger));
    }
    to_bytes(&root, &[], &[], true).map_err(|_| NativeError::Unwritable("the state part cannot be written"))
}
