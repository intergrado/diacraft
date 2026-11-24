//! The ties of containers and lists (SHAPEOPS-CONTAINERS): which shapes a shape is a member of, and the places a produced shape takes.
//!
//! A shape's ties are the `DEPENDSON` calls of its `Relationships` cell (`calc`'s `fn_cells.rs`): `DEPENDSON(1, Sheet.5!SheetRef(), ...)` is a container
//! that holds those shapes, 2 a list that does, 4 a container the shape is in, 5 a list it is in; 3 and 6 are callouts. Our rule (SHAPEOPS-CONTAINERS):
//!
//! * a shape that holds members, or has any `User.msvStructureType` of `Container` or `List`, takes no part in an operation;
//! * a produced shape, when the command makes one, takes the places of the first shape: it holds the first shape's ties of kinds 4 and 5, and each container or
//!   list that named the first shape names it instead. The other originals leave their places, and the ties that name them name nothing (a lost target is no
//!   target), as a delete leaves them;
//! * callouts are not carried, and what ties them to a shape that goes is lost the same way.

use std::collections::HashMap;

use diacraft_doc::{Cell, CellAddr, Document, FormulaSlot, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, ShapeList, SheetRef, Value};
use diacraft_formula::Expr;

use crate::draw::remap::rewrite_refs;
use crate::draw::{Problem, Reader, shape_mut};

/// The code of the ties that say a shape is in a container, and in a list.
const IN_CONTAINER: u32 = 4;
const IN_LIST: u32 = 5;

/// The `DEPENDSON` calls of the shape's `Relationships` cell, as the code and the call: the cell's own formula or its master's, walked and not evaluated.
fn calls(reader: &Reader<'_>, sheet: &SheetRef) -> Vec<(u32, Expr)> {
    let Some(found) = reader.resolver().resolve(sheet, &CellAddr::direct("Relationships")) else { return Vec::new() };
    let Some(FormulaSlot::Local(formula)) = found.formula.cell.map(|c| &c.formula) else { return Vec::new() };
    let Ok(root) = &formula.ast else { return Vec::new() };
    let mut out = Vec::new();
    let mut stack: Vec<&Expr> = vec![root];
    let mut steps = 0_usize;
    while let Some(e) = stack.pop() {
        steps += 1;
        if steps > 4_096 {
            break;
        }
        match e {
            Expr::Call(c) if c.is("DEPENDSON") && c.qualifiers.is_empty() => {
                let code = match c.args.first() {
                    Some(Some(Expr::Number(n))) => n.quantity().map(|q| q.stored),
                    _ => None,
                };
                if let Some(code) = code.filter(|c| (0.0..1.0e6).contains(c)) {
                    out.push((code as u32, e.clone()));
                }
            }
            Expr::Call(c) => stack.extend(c.args.iter().rev().flatten()),
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(b);
                stack.push(a);
            }
            _ => {}
        }
    }
    out
}

/// Whether the shape holds members, as a container or a list (a tie of kind 1 or 2).
pub(super) fn holds_members(reader: &Reader<'_>, sheet: &SheetRef) -> bool {
    calls(reader, sheet).iter().any(|(code, _)| matches!(code, 1 | 2))
}

/// The `Relationships` cell a produced shape takes from the first shape: only its ties of kinds 4 and 5, as `SUM(DEPENDSON(4, ...), ...)`; `None` when it has none.
pub(super) fn carried(reader: &Reader<'_>, sheet: &SheetRef) -> Option<Cell> {
    let kept: Vec<String> = calls(reader, sheet)
        .into_iter()
        .filter(|(code, _)| matches!(*code, IN_CONTAINER | IN_LIST))
        .map(|(_, call)| diacraft_formula::print(&call))
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(Cell::with_formula(&format!("SUM({})", kept.join(",")), Some(Value::from_text("0"))))
}

/// The formula with `Sheet.<from>!` naming `Sheet.<to>!` inside the calls that hold members, kinds 1 and 2, and nowhere else: a callout's tie (kind 3 or 6) to a shape
/// that goes is left to name nothing. `None` when nothing is named. The calls are found in the text: they do not nest, so each runs to the next.
fn rewrite_members(source: &str, map: &HashMap<u32, u32>) -> Option<String> {
    let lower = source.to_ascii_lowercase();
    let starts: Vec<usize> = lower.match_indices("dependson(").map(|(at, _)| at).collect();
    let first = *starts.first()?;
    let mut out = String::with_capacity(source.len());
    out.push_str(source.get(..first)?);
    let mut changed = false;
    for (k, start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(source.len());
        let segment = source.get(*start..end)?;
        let digits: String = segment.get("dependson(".len()..).unwrap_or_default().trim_start().chars().take_while(char::is_ascii_digit).collect();
        let holds_members = matches!(digits.parse::<u32>(), Ok(1 | 2));
        match holds_members.then(|| rewrite_refs(segment, map)).flatten() {
            Some(new) => {
                out.push_str(&new);
                changed = true;
            }
            None => out.push_str(segment),
        }
    }
    changed.then_some(out)
}

/// Puts the shape `to` where the shape `from` was in every container and list of the page: each `Relationships` formula that holds `Sheet.<from>!` as a member
/// holds `Sheet.<to>!`. One walk of the page, no deeper than the model allows and at most [`MAX_SHAPES_PER_PAGE`] shapes.
pub(super) fn retarget(doc: &mut Document, page: u32, from: u32, to: u32) -> Result<(), Problem> {
    let map: HashMap<u32, u32> = [(from, to)].into_iter().collect();
    let mut hits: Vec<(Vec<u32>, String)> = Vec::new();
    {
        let Some(p) = doc.page(page) else { return Ok(()) };
        let mut stack: Vec<(&ShapeList, usize)> = vec![(&p.shapes, 0)];
        let mut path: Vec<u32> = Vec::new();
        let mut nodes = 0_usize;
        while let Some(frame) = stack.last_mut() {
            let (list, at) = *frame;
            let Some(shape) = list.get(at) else {
                stack.pop();
                continue;
            };
            frame.1 = at.saturating_add(1);
            nodes += 1;
            if nodes > MAX_SHAPES_PER_PAGE {
                return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
            }
            let depth = stack.len();
            path.truncate(depth.saturating_sub(1));
            path.push(u32::try_from(at).unwrap_or(u32::MAX));
            if let Some(FormulaSlot::Local(formula)) = shape.sheet.cell("Relationships").map(|c| &c.formula)
                && let Some(rewritten) = rewrite_members(&formula.source, &map)
            {
                hits.push((path.clone(), rewritten));
            }
            if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                stack.push((&shape.children, 0));
            }
        }
    }
    for (path, source) in hits {
        let shape = shape_mut(doc, page, &path).ok_or_else(|| Problem::failed("a shape went away"))?;
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        if let Some(old) = sheet.cells.get_mut("Relationships") {
            let mut cell = Cell::with_formula(&source, old.value.clone());
            cell.unit = old.unit.clone();
            *old = cell;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn to_nine() -> HashMap<u32, u32> {
        [(1, 9)].into_iter().collect()
    }

    #[test]
    fn a_member_list_names_the_new_shape_and_nothing_else_does() {
        let source = "SUM(DEPENDSON(1,Sheet.1!SheetRef(),Sheet.2!SheetRef()),DEPENDSON(6,Sheet.1!SheetRef()),DEPENDSON(2,Sheet.3!SheetRef(),Sheet.1!SheetRef()))";
        let rewritten = rewrite_members(source, &to_nine()).unwrap();
        assert_eq!(
            rewritten,
            "SUM(DEPENDSON(1,Sheet.9!SheetRef(),Sheet.2!SheetRef()),DEPENDSON(6,Sheet.1!SheetRef()),DEPENDSON(2,Sheet.3!SheetRef(),Sheet.9!SheetRef()))",
            "kinds 1 and 2 name the new shape, a callout's tie (6) is left"
        );
    }

    #[test]
    fn a_tie_that_is_not_a_member_list_a_formula_that_names_nobody_and_text_that_is_not_a_call_are_left_alone() {
        assert_eq!(rewrite_members("SUM(DEPENDSON(4,Sheet.1!SheetRef()))", &to_nine()), None, "kind 4 is the shape's own place");
        assert_eq!(rewrite_members("SUM(DEPENDSON(1,Sheet.2!SheetRef()))", &to_nine()), None, "it names another shape");
        assert_eq!(rewrite_members("Sheet.1!PinX", &to_nine()), None, "no tie at all");
        assert_eq!(rewrite_members("", &to_nine()), None);
        assert_eq!(rewrite_members("SUM(DEPENDSON(,Sheet.1!SheetRef()))", &to_nine()), None, "a call with no kind");
    }

    #[test]
    fn the_calls_are_found_in_any_case_and_with_blanks_before_the_kind() {
        let source = "SUM(dependson( 1,Sheet.1!SheetRef()),DependsOn(2 ,Sheet.1!SheetRef()))";
        assert_eq!(rewrite_members(source, &to_nine()).unwrap(), "SUM(dependson( 1,Sheet.9!SheetRef()),DependsOn(2 ,Sheet.9!SheetRef()))");
    }
}
