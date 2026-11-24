//! The comments of the drawing against the comments of the file read back (class `comments`).
//!
//! **One direction, as every class is:** each comment the drawing holds is looked for in the file's (by its number: every format keeps `CommentID`, `diacraft:id` or the codec's number),
//! and one that is not there, or is there with another author, text, date, edit date, Done mark, thread or place, is a comment that differs. A comment the file holds beyond the
//! drawing's is no loss. **The place is the page's position in the drawing and the shape's place among the page's shapes** (pre-order), and not the numbers they have: a format
//! may number pages and shapes again (the classes `pages` and `shape-numbers` say so), and a comment is no worse for it when it is on the same shape. A comment on a shape the
//! drawing no longer has is the page's. The text is the same with its line separators as line breaks.
//!
//! **Cost.** One step for each comment, page and author of each drawing, and one for each shape of a page that has a comment on a shape, charged before the table of each is made.

use std::collections::HashMap;

use diacraft_doc::{Comment, Document, Page, Shape};

use super::meter::Stop;

/// The place of each shape of `page` among its shapes, in pre-order, by `ID`: the first shape of the page that has an `ID` is the one the number names.
fn order(page: &Page, step: &mut dyn FnMut(usize) -> Result<(), Stop>) -> Result<HashMap<u32, usize>, Stop> {
    let mut out: HashMap<u32, usize> = HashMap::new();
    let mut stack: Vec<&Shape> = page.shapes.iter().rev().map(|s| &**s).collect();
    let mut at = 0_usize;
    while let Some(shape) = stack.pop() {
        step(1)?;
        out.entry(shape.id).or_insert(at);
        at += 1;
        stack.extend(shape.children.iter().rev().map(|s| &**s));
    }
    Ok(out)
}

/// The place of each shape among its page's shapes, by `ID`, for each page that has a comment on a shape.
type Orders = HashMap<u32, HashMap<u32, usize>>;

fn lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace(['\r', '\u{2028}'], "\n")
}

/// How many comments of `before` are not in `after` as they are.
pub(crate) fn differing(before: &Document, after: &Document, step: &mut dyn FnMut(usize) -> Result<(), Stop>) -> Result<u64, Stop> {
    let (wanted, found) = (&before.comments, &after.comments);
    if wanted.is_empty() {
        return Ok(0);
    }
    step(wanted.len().saturating_add(found.len()).saturating_add(before.pages.len()).saturating_add(after.pages.len()))?;
    let by_id: HashMap<u32, &Comment> = found.iter().map(|c| (c.id, &**c)).collect();
    let names = |doc: &Document| -> HashMap<u32, (String, String)> {
        doc.comments.authors().map(|a| (a.id, (a.name.to_string(), a.initials.to_string()))).collect()
    };
    let (names_before, names_after) = (names(before), names(after));
    step(names_before.len().saturating_add(names_after.len()))?;
    let at_of = |doc: &Document| -> HashMap<u32, usize> { doc.pages.iter().enumerate().map(|(at, p)| (p.id, at)).collect() };
    let (pages_before, pages_after) = (at_of(before), at_of(after));
    // The order of the shapes of a page, made once for a page that has a comment on a shape.
    let mut orders: (Orders, Orders) = (HashMap::new(), HashMap::new());
    let mut differs = 0_u64;
    for c in wanted.iter() {
        let Some(f) = by_id.get(&c.id).copied() else {
            differs += 1;
            continue;
        };
        let (Some(pb), Some(pa)) = (pages_before.get(&c.page).copied(), pages_after.get(&f.page).copied()) else {
            differs += 1;
            continue;
        };
        let author = names_before.get(&c.author).cloned().unwrap_or_default() == names_after.get(&f.author).cloned().unwrap_or_default();
        let mut same = pb == pa
            && author
            && lines(&c.text) == lines(&f.text)
            && c.date == f.date
            && c.edit_date == f.edit_date
            && c.done == f.done
            && c.parent == f.parent;
        if same && (c.shape.is_some() || f.shape.is_some()) {
            // The shape each names, by its place among its page's shapes; a shape the drawing no longer has names none.
            let place = |doc: &Document,
                         orders: &mut Orders,
                         at: usize,
                         shape: Option<u32>,
                         step: &mut dyn FnMut(usize) -> Result<(), Stop>|
             -> Result<Option<usize>, Stop> {
                let Some(shape) = shape else { return Ok(None) };
                let Some(page) = doc.pages.get(at) else { return Ok(None) };
                if let std::collections::hash_map::Entry::Vacant(slot) = orders.entry(page.id) {
                    slot.insert(order(page, step)?);
                }
                Ok(orders.get(&page.id).and_then(|m| m.get(&shape)).copied())
            };
            let was = place(before, &mut orders.0, pb, c.shape, step)?;
            let now = place(after, &mut orders.1, pa, f.shape, step)?;
            same = was == now;
        }
        if !same {
            differs += 1;
        }
    }
    Ok(differs)
}
