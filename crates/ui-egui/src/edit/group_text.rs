//! Where text typed on a group goes.
//!
//! A group's `IsTextEditTarget` says whether the group takes the text typed on it (TRUE) or sends it to the member on top of the stack, the last in
//! document order (FALSE). The cell is found by inheritance and reads TRUE where nothing gives one (the root style holds 1). **The window reads it
//! when an edit begins** (the cell changes where later typing goes, and moves no text that is already there), so an edit that is open stays where
//! it opened. The members hold the text of a group made from a master of stacked parts, such as a class with its name, attributes and operations:
//! typing on the selected class edits its name, the last of its members (design call of the second stencil batch).
//!
//! **Assumed (no specification says): the walk goes on through a member that is a group with its own cell FALSE**, as it does through the first, so
//! that a group of groups ends at a shape that holds text; a member group whose cell is TRUE takes the text itself. A member that is not drawn (deleted
//! from its master, or a guide) is passed over: typing cannot be seen there.

use diacraft_engine::doc::{CellAddr, Document, Owner, Resolver, ShapeRef, ShapeType, SheetRef};

use super::scene::MAX_GROUP_DEPTH;

/// The most members looked at, counted from the end of a group's list, for one that is drawn: a group of many deleted shapes is no hang.
const MAX_SCAN: usize = 5_000;

/// Whether a cell holding `value` reads TRUE: a number that is not 0, or any word that is not `FALSE`. Nothing at all is TRUE, the default.
fn reads_true(value: Option<&str>) -> bool {
    let Some(text) = value.map(str::trim) else { return true };
    match text.parse::<f64>() {
        Ok(n) if n.is_finite() => n != 0.0,
        _ => !text.eq_ignore_ascii_case("false"),
    }
}

/// Whether text typed on the group at `at` goes to the group itself: its `IsTextEditTarget`, found by inheritance, reads TRUE.
fn takes_text_itself(res: &Resolver<'_>, at: &ShapeRef) -> bool {
    let found = res.resolve(&SheetRef::Shape(at.clone()), &CellAddr::direct("IsTextEditTarget"));
    let value = found.as_ref().and_then(|found| found.value.cell).and_then(|cell| cell.value.as_ref());
    reads_true(value.map(|value| value.text()))
}

/// The shape that text typed on shape `id` of page `page` reaches: `id` itself, unless it is a group whose `IsTextEditTarget` is FALSE, and then the
/// member on top of the stack (the last that is drawn), and again, down through the groups among them, for as many levels as the pointer reads
/// ([`MAX_GROUP_DEPTH`]). A shape the page does not have, a shape that is no group and a group with no member that is drawn are `id`.
pub(super) fn text_target(doc: &Document, page: u32, id: u32) -> u32 {
    let owner = Owner::Page(page);
    let Some(mut at) = doc.find_shape(owner, id) else { return id };
    let res = Resolver::new(doc);
    for _ in 0..MAX_GROUP_DEPTH {
        let Some(shape) = doc.shape(&at) else { break };
        if shape.kind != ShapeType::Group || takes_text_itself(&res, &at) {
            break;
        }
        let last = shape.children.iter().enumerate().rev().take(MAX_SCAN).find(|(_, member)| !member.del && member.kind != ShapeType::Guide);
        let Some(index) = last.and_then(|(index, _)| u32::try_from(index).ok()) else { break };
        at = at.child(index);
    }
    // An `ID` that a hostile file repeats names the first shape that has it, not this one: the text would go to another shape, so it stays with `id`.
    match doc.shape(&at) {
        Some(shape) if doc.find_shape(owner, shape.id).as_ref() == Some(&at) => shape.id,
        _ => id,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use diacraft_engine::Session;
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn a_cell_reads_true_unless_it_is_zero_or_false() {
        assert!(reads_true(None), "no cell: the root style's 1");
        for text in ["1", "1.0", "-1", "2", "TRUE", "true", "yes", ""] {
            assert!(reads_true(Some(text)), "{text:?}");
        }
        for text in ["0", "0.0", "-0", " 0 ", "FALSE", "false", "False"] {
            assert!(!reads_true(Some(text)), "{text:?}");
        }
        assert!(reads_true(Some("NaN")), "a number that is not finite is not a 0");
    }

    fn page_of(s: &Session) -> u32 {
        s.document().unwrap().pages.get(0).unwrap().id
    }

    fn run(s: &mut Session, command: &str, params: &Value) -> Value {
        s.execute(command, params).unwrap_or_else(|e| panic!("`{command}` failed: {e}"))
    }

    fn boxed(s: &mut Session, y: f64) -> u32 {
        let page = page_of(s);
        u32::try_from(run(s, "shape.drop", &json!({"master": "rectangle", "page": page, "x": 4.0, "y": y}))["id"].as_u64().unwrap()).unwrap()
    }

    fn group(s: &mut Session, ids: &[u32]) -> u32 {
        let page = page_of(s);
        u32::try_from(run(s, "shape.group", &json!({"ids": ids, "page": page}))["id"].as_u64().unwrap()).unwrap()
    }

    fn set(s: &mut Session, id: u32, formula: &str) {
        let page = page_of(s);
        run(s, "sheet.set_cell", &json!({"id": id, "page": page, "cell": "IsTextEditTarget", "formula": formula}));
    }

    fn target(s: &Session, id: u32) -> u32 {
        text_target(s.document().unwrap(), page_of(s), id)
    }

    #[test]
    fn a_group_sends_text_to_its_last_member_unless_its_cell_is_true() {
        let mut s = Session::new();
        run(&mut s, "file.new", &json!({}));
        let (a, b, c) = (boxed(&mut s, 3.0), boxed(&mut s, 4.0), boxed(&mut s, 5.0));
        let g = group(&mut s, &[a, b, c]);
        assert_eq!(target(&s, g), g, "no cell: the group");
        set(&mut s, g, "0");
        assert_eq!(target(&s, g), c, "FALSE: the member on top of the stack, the last");
        set(&mut s, g, "TRUE");
        assert_eq!(target(&s, g), g);
        set(&mut s, g, "FALSE");
        assert_eq!(target(&s, g), c);
        assert_eq!(target(&s, a), a, "a shape that is no group is itself");
        assert_eq!(target(&s, 999), 999, "a shape the page does not have is its own id");
        assert_eq!(text_target(s.document().unwrap(), 999, g), g, "and so is a shape of a page it does not have");
    }

    #[test]
    fn a_group_of_groups_ends_at_a_shape_that_holds_text() {
        let mut s = Session::new();
        run(&mut s, "file.new", &json!({}));
        let first = boxed(&mut s, 7.0);
        let (below, last) = (boxed(&mut s, 6.0), boxed(&mut s, 5.0));
        let inner = group(&mut s, &[below, last]);
        let outer = group(&mut s, &[first, inner]);
        set(&mut s, outer, "0");
        assert_eq!(target(&s, outer), inner, "the inner group has no cell: it takes the text");
        set(&mut s, inner, "0");
        assert_eq!(target(&s, outer), last);
        assert_eq!(target(&s, inner), last);
    }

    /// Groups nested past the depth the pointer reads end the walk where it has got to: no loop is unbounded (never crash), and the text goes to a shape
    /// of the chain.
    #[test]
    fn a_chain_of_groups_deeper_than_the_walk_goes_ends_at_a_group_of_the_chain() {
        let mut s = Session::new();
        run(&mut s, "file.new", &json!({}));
        let levels = MAX_GROUP_DEPTH + 4;
        // The outer boxes first, so that each is before the group it joins in the page's order, and the group is the last member.
        let outers: Vec<u32> = (0..levels).map(|n| boxed(&mut s, 10.0 - 0.5 * n as f64)).collect();
        let (x, y) = (boxed(&mut s, 1.0), boxed(&mut s, 0.5));
        let mut inner = group(&mut s, &[x, y]);
        set(&mut s, inner, "0");
        let mut chain = vec![inner];
        for outer in outers.iter().rev() {
            inner = group(&mut s, &[*outer, inner]);
            set(&mut s, inner, "0");
            chain.push(inner);
        }
        let top = *chain.last().unwrap();
        let reached = target(&s, top);
        assert!(chain.contains(&reached), "{reached} is a group of the chain");
        assert_ne!(reached, top, "and the walk went some levels down");
        assert_ne!(reached, y, "but not the whole way: the depth is capped");
        // From a group closer to the bottom the walk gets all the way.
        assert_eq!(target(&s, chain[1]), y);
    }
}
