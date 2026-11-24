//! The walk of Find: the shapes of a scope, in document order, once each.
//!
//! **Document order** is the pages in the document's order, and on a page a group before its members, each member in its place in the group (the order of
//! `Document::find_shape`). The walk is iterative, goes no deeper than the model allows ([`MAX_SHAPE_DEPTH`]), visits at most [`MAX_SHAPES_PER_PAGE`] shapes on a
//! page, and is charged a few steps for each shape, so a hostile drawing cannot make it long. A shape that is deleted from its master (`Del`) and a guide are not
//! visited: neither is content.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{Document, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Shape, ShapeList, ShapeType};

use super::pattern::{Refused, Work};

/// Steps a shape costs to be passed.
pub const SHAPE_STEPS: u64 = 8;

/// Where Find looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The shapes `ids` of one page, and their members.
    Selection { page: u32, ids: Vec<u32> },
    /// One page.
    Page(u32),
    /// Every page, in order.
    All,
}

/// Whether the walk goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Next,
    Stop,
}

/// A shape the walk is at.
pub struct Visit<'a, 'p> {
    /// The `ID` of its page.
    pub page: u32,
    /// Where it is on the page: the position of each shape on the way down to it.
    pub path: &'p [u32],
    pub shape: &'a Arc<Shape>,
}

/// Walks the shapes of `scope` in document order, calling `f` with each. A page that is not there is an error for [`Scope::Page`] and [`Scope::Selection`]; the caller
/// checks that first, so it is not one here. `f` may charge `work` too. → how many shapes were visited.
pub fn walk<'a>(
    doc: &'a Document,
    scope: &Scope,
    work: &mut Work,
    f: &mut dyn FnMut(&Visit<'a, '_>, &mut Work) -> Result<Flow, Refused>,
) -> Result<u64, Refused> {
    let mut visited = 0_u64;
    let selected: HashSet<u32> = match scope {
        Scope::Selection { ids, .. } => ids.iter().copied().collect(),
        _ => HashSet::new(),
    };
    let only_selected = matches!(scope, Scope::Selection { .. });
    for page in doc.pages.iter() {
        let wanted = match scope {
            Scope::All => true,
            Scope::Page(id) | Scope::Selection { page: id, .. } => page.id == *id,
        };
        if !wanted {
            continue;
        }
        // One frame for each level: the list being walked, the place of the next shape in it, and whether the list is inside a selected shape.
        let mut stack: Vec<(&ShapeList, usize, bool)> = vec![(&page.shapes, 0, false)];
        let mut path: Vec<u32> = Vec::new();
        let mut on_page = 0_usize;
        while let Some(frame) = stack.last_mut() {
            let (list, at, inside) = *frame;
            let Some(shape) = list.get(at) else {
                stack.pop();
                continue;
            };
            frame.1 = at.saturating_add(1);
            on_page = on_page.saturating_add(1);
            work.charge(SHAPE_STEPS)?;
            if on_page > MAX_SHAPES_PER_PAGE {
                break;
            }
            let depth = stack.len();
            path.truncate(depth.saturating_sub(1));
            path.push(u32::try_from(at).unwrap_or(u32::MAX));
            let selected_here = inside || selected.contains(&shape.id);
            let is_content = !shape.del && shape.kind != ShapeType::Guide;
            if is_content && (!only_selected || selected_here) {
                visited = visited.saturating_add(1);
                if f(&Visit { page: page.id, path: &path, shape }, work)? == Flow::Stop {
                    return Ok(visited);
                }
            }
            if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                stack.push((&shape.children, 0, selected_here));
            }
        }
    }
    Ok(visited)
}
