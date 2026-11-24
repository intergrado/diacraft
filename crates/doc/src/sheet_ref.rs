//! Naming a sheet of a document, and getting at it.
//!
//! A shape is named by where it sits (a path of child positions), not by `ID`: IDs are not unique in real files
//! And a path always names one shape. A [`SheetRef`] is a handle for a query, not a stored link;
//! a structural edit that moves shapes makes older paths name other shapes, so a command looks a shape up by `ID`
//! ([`Document::find_shape`]) when it runs.

use std::sync::Arc;

use crate::document::Document;
use crate::shape::{MAX_SHAPE_DEPTH, Shape, ShapeList};
use crate::sheet::Sheet;

/// What holds a tree of shapes: a page part or a master part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The page with this `ID` (the first, if a hostile file repeats it).
    Page(u32),
    /// The master with this `ID`.
    Master(u32),
}

/// A shape: the part that holds it, and the position of each shape on the way down to it, from the part's own list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeRef {
    pub owner: Owner,
    /// `[3, 0]` is the first member of the fourth top-level shape. Never longer than [`MAX_SHAPE_DEPTH`] to be valid.
    pub path: Vec<u32>,
}

impl ShapeRef {
    /// The top-level shape at `at` of `owner`.
    pub fn top(owner: Owner, at: u32) -> ShapeRef {
        ShapeRef { owner, path: vec![at] }
    }

    /// The `at`th member of this shape.
    pub fn child(&self, at: u32) -> ShapeRef {
        let mut path = self.path.clone();
        path.push(at);
        ShapeRef { owner: self.owner, path }
    }

    /// The shape this one is a member of, or `None` for a top-level shape.
    pub fn parent(&self) -> Option<ShapeRef> {
        let (_, up) = self.path.split_last()?;
        (!up.is_empty()).then(|| ShapeRef { owner: self.owner, path: up.to_vec() })
    }
}

/// A sheet of a document.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SheetRef {
    /// The `DocumentSheet`.
    Document,
    /// The style with this `ID`.
    Style(u32),
    /// A page's `PageSheet`.
    Page(u32),
    /// A master's `PageSheet`.
    Master(u32),
    /// A shape.
    Shape(ShapeRef),
}

impl Document {
    /// The sheet `at` names, if the document has it.
    pub fn sheet_of(&self, at: &SheetRef) -> Option<&Arc<Sheet>> {
        match at {
            SheetRef::Document => Some(&self.sheet),
            SheetRef::Style(id) => self.style(*id).map(|s| &s.sheet),
            SheetRef::Page(id) => self.page(*id).map(|p| &p.sheet),
            SheetRef::Master(id) => self.master(*id).map(|m| &m.sheet),
            SheetRef::Shape(shape) => self.shape(shape).map(|s| &s.sheet),
        }
    }

    /// The shapes of a page or master part.
    pub fn shapes_of(&self, owner: Owner) -> Option<&ShapeList> {
        match owner {
            Owner::Page(id) => self.page(id).map(|p| &p.shapes),
            Owner::Master(id) => self.master(id).map(|m| &m.shapes),
        }
    }

    /// The shape `at` names. `None` for a path that leads nowhere or is deeper than [`MAX_SHAPE_DEPTH`].
    pub fn shape(&self, at: &ShapeRef) -> Option<&Arc<Shape>> {
        shape_in(self.shapes_of(at.owner)?, &at.path)
    }

    /// The shapes from the top-level shape down to the one `at` names, as many as its path is long: each is the parent of the
    /// next. `None` if any step leads nowhere.
    pub fn shape_chain(&self, at: &ShapeRef) -> Option<Vec<&Arc<Shape>>> {
        chain_in(self.shapes_of(at.owner)?, &at.path)
    }

    /// The first shape with `id` in a page or master part, in document order, a group's members after the group and before
    /// the next shape: where a formula's `Sheet.<id>` lands when it is not inherited (a part may hold an `ID` twice). The
    /// walk is iterative and stops at [`MAX_SHAPE_DEPTH`] levels.
    pub fn find_shape(&self, owner: Owner, id: u32) -> Option<ShapeRef> {
        find_shape_in(self.shapes_of(owner)?, id).map(|path| ShapeRef { owner, path })
    }
}

/// The shape at the end of `path` in a tree of shapes. `None` for a path that leads nowhere or is deeper than [`MAX_SHAPE_DEPTH`].
pub(crate) fn shape_in<'a>(list: &'a ShapeList, path: &[u32]) -> Option<&'a Arc<Shape>> {
    chain_in(list, path)?.pop()
}

/// The shapes down a path in a tree of shapes, each the parent of the next; the one place the model follows a path, whoever asks
/// (the document and the resolver, which only differ in how they find the part).
pub(crate) fn chain_in<'a>(top: &'a ShapeList, path: &[u32]) -> Option<Vec<&'a Arc<Shape>>> {
    if path.len() > MAX_SHAPE_DEPTH || path.is_empty() {
        return None;
    }
    let mut list = top;
    let mut chain = Vec::with_capacity(path.len());
    for step in path {
        let shape = list.get(usize::try_from(*step).ok()?)?;
        chain.push(shape);
        list = &shape.children;
    }
    Some(chain)
}

/// What a walk of a shape tree does at a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Visit {
    /// This is the shape wanted.
    Match,
    /// Not it: go on into its members.
    Descend,
    /// Not it, and its members are not looked at.
    Skip,
}

/// Walks a tree of shapes depth first in document order, without recursion, calling `visit` with each shape and the path to it,
/// until it matches: the path to that shape. Descends [`MAX_SHAPE_DEPTH`] levels and no more.
pub(crate) fn walk(top: &ShapeList, mut visit: impl FnMut(&Shape, &[u32]) -> Visit) -> Option<Vec<u32>> {
    // One frame per level: the list being walked and the position of the next shape in it.
    let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 0)];
    // The position of the shape being visited at each level above and including this one.
    let mut path: Vec<u32> = Vec::new();
    while let Some(frame) = stack.last_mut() {
        let (list, at) = *frame;
        let Some(shape) = list.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        path.truncate(stack.len().saturating_sub(1));
        path.push(u32::try_from(at).unwrap_or(u32::MAX));
        match visit(shape.as_ref(), &path) {
            Visit::Match => return Some(path),
            Visit::Descend if stack.len() < MAX_SHAPE_DEPTH => stack.push((&shape.children, 0)),
            Visit::Descend | Visit::Skip => {}
        }
    }
    None
}

/// The path to the first shape that `visit` matches, as [`walk`] finds it.
pub(crate) fn find_path(top: &ShapeList, mut visit: impl FnMut(&Shape) -> Visit) -> Option<Vec<u32>> {
    walk(top, |shape, _| visit(shape))
}

/// The path to the first shape with `id` in a tree of shapes.
pub(crate) fn find_shape_in(top: &ShapeList, id: u32) -> Option<Vec<u32>> {
    find_path(top, |shape| {
        crate::work::find_visit();
        if shape.id == id { Visit::Match } else { Visit::Descend }
    })
}
