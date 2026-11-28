//! What a double click on a shape does, as far as its `EventDblClick` says (OPENGROUPWIN).
//!
//! The shape's `EventDblClick` is read where the sheet finds it (the shape, its master, a style), and **never worked out**: an action cell is data, so the
//! formula is looked at for the one call this module is about. `OPENGROUPWIN()` (and its old spelling `_OPENGROUPWIN()`) opens a group in a window of its own,
//! where its members can be edited apart. **No group window exists in DiaCraft** (a project rule): a double click on a **group** whose cell calls it
//! opens nothing, and the status bar says so ([`NO_GROUP_WINDOWS`]); on a shape that is no group the call does nothing, and says nothing (no error for it). Either way the
//! call stands in for the double click's default, so no text box opens.
//!
//! A cell that does not call it, one that does not parse and a shape with none are the default: the text of the shape is edited ([`DoubleClick::EditText`]). The formula is
//! walked with a stack of its own, so a tree of any depth the parser makes is looked at without recursion.

use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, ShapeType, SheetRef};
use diacraft_formula::Expr;

/// What the status bar says when a double click would open a group's window: that it would, and that there are none.
pub const NO_GROUP_WINDOWS: &str = "Double clicking this group opens a group window, and DiaCraft has no group windows, so nothing was opened.";

/// What a double click on a shape comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoubleClick {
    /// The default: the shape's text is edited (the shape's own words, or the member that holds them: `IsTextEditTarget`).
    EditText,
    /// The cell calls `OPENGROUPWIN()` on a group: nothing opens, and [`NO_GROUP_WINDOWS`] is said.
    GroupWindow,
    /// The cell calls `OPENGROUPWIN()` on a shape that is no group: nothing opens, and nothing is said.
    Nothing,
}

/// Whether the formula calls `OPENGROUPWIN` (or `_OPENGROUPWIN`) anywhere in it.
fn calls_open_group_window(expr: &Expr) -> bool {
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Call(c) => {
                if c.qualifiers.is_empty() && (c.is("OPENGROUPWIN") || c.is("_OPENGROUPWIN")) {
                    return true;
                }
                stack.extend(c.args.iter().flatten());
            }
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            _ => {}
        }
    }
    false
}

/// What a double click on shape `id` of page `page` does. A shape the page does not have is the default.
pub fn on_double_click(doc: &Document, page: u32, id: u32) -> DoubleClick {
    let Some(at) = doc.find_shape(Owner::Page(page), id) else { return DoubleClick::EditText };
    let Some(shape) = doc.shape(&at) else { return DoubleClick::EditText };
    let res = Resolver::new(doc);
    let found = res.resolve(&SheetRef::Shape(at), &CellAddr::direct("EventDblClick"));
    let Some(FormulaSlot::Local(formula)) = found.as_ref().and_then(|f| f.formula.cell).map(|c| &c.formula) else { return DoubleClick::EditText };
    match &formula.ast {
        Ok(expr) if calls_open_group_window(expr) => {
            if shape.kind == ShapeType::Group {
                DoubleClick::GroupWindow
            } else {
                DoubleClick::Nothing
            }
        }
        _ => DoubleClick::EditText,
    }
}
