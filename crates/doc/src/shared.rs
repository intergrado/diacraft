//! [`SharedVec`]: a list that is cheap to clone and cheap to edit one element of.
//!
//! A page of 10,000 shapes held in a `Vec<Arc<Shape>>` costs 10,000 reference counts every time the page is cloned,
//! which is what every edit and every undo step does. Held here, the same page is 157 chunks of up to [`CHUNK`]
//! elements behind one pointer: **a clone copies that one pointer**, and replacing one shape copies the list of 157
//! chunk pointers and that shape's chunk (64 reference counts), nothing else. So a list an edit does not write to costs
//! an undo step one pointer however long it is (65,536 styles are 1,024 chunks, and a step that edits a shape copies none
//! of them), and a thousand steps hold a few hundred kilobytes of list, not tens of megabytes.
//!
//! The layout is canonical: every chunk is full but the last, whatever sequence of pushes, inserts and removes made the
//! list. So two lists with the same elements have the same chunks, and `==` is content equality. Inserting or removing in
//! the middle rebuilds the chunks from the one that holds the place to the end (the ones before it stay shared), which is
//! linear in the elements after it; the common edits, replacing and pushing, are not.

use std::sync::Arc;

/// Most elements in one chunk. Chosen for a page of ten to forty thousand shapes: the top list and a chunk then cost about
/// the same to copy (see the module documentation).
pub const CHUNK: usize = 64;

/// A place past the end of a [`SharedVec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("that place is past the end of the list")]
pub struct OutOfRange;

/// A persistent list: see the [module documentation](self).
#[derive(Debug)]
pub struct SharedVec<T> {
    /// The chunks, behind one pointer; none for an empty list, which most shapes' members are, so it costs no allocation.
    top: Option<Arc<Vec<Arc<Vec<T>>>>>,
    len: usize,
}

impl<T> Clone for SharedVec<T> {
    fn clone(&self) -> Self {
        SharedVec { top: self.top.clone(), len: self.len }
    }
}

impl<T> Default for SharedVec<T> {
    fn default() -> Self {
        SharedVec { top: None, len: 0 }
    }
}

impl<T: PartialEq> PartialEq for SharedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        // The layout is canonical, so equal content is equal chunks.
        self.len == other.len && self.chunks().iter().zip(other.chunks()).all(|(a, b)| Arc::ptr_eq(a, b) || a == b)
    }
}

impl<T> SharedVec<T> {
    /// An empty list.
    pub fn new() -> Self {
        SharedVec::default()
    }

    /// How many elements.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn chunks(&self) -> &[Arc<Vec<T>>] {
        self.top.as_deref().map_or(&[], Vec::as_slice)
    }

    /// The chunks themselves, each behind the pointer it is shared by: what a count of the memory two lists hold between them tells chunks apart by
    /// (`Arc::as_ptr`), where `shared_chunks_with` compares only the same place of two lists.
    pub fn chunk_arcs(&self) -> &[Arc<Vec<T>>] {
        self.chunks()
    }

    /// The element at `at`, or `None` past the end.
    pub fn get(&self, at: usize) -> Option<&T> {
        self.chunks().get(at / CHUNK)?.get(at % CHUNK)
    }

    /// The list of chunks that a clone of this list shares, behind its one pointer (`None` for an empty list): what a count of the memory several documents hold
    /// between them looks at first, as a document's pages, masters, styles and a shape's members are held by it (the engine's count of what a session holds).
    pub fn top_arc(&self) -> Option<&Arc<Vec<Arc<Vec<T>>>>> {
        self.top.as_ref()
    }

    /// The elements in order, from either end.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter { list: self, front: 0, back: self.len }
    }

    /// How many chunks (the number of pointers a write to the list copies, besides the one chunk it writes to).
    pub fn chunk_count(&self) -> usize {
        self.chunks().len()
    }

    /// How many of this list's chunks are the very same allocation as the chunk in the same place of `other`: what an
    /// edit left shared with an older copy.
    pub fn shared_chunks_with(&self, other: &SharedVec<T>) -> usize {
        self.chunks().iter().zip(other.chunks()).filter(|(a, b)| Arc::ptr_eq(a, b)).count()
    }

    /// Whether this is the very list `other` is: no write has copied it since the two were one. A clone is, until one of them is written.
    pub fn is_same_list_as(&self, other: &SharedVec<T>) -> bool {
        match (&self.top, &other.top) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }

    fn from_chunks_of(items: Vec<T>) -> Vec<Arc<Vec<T>>> {
        let mut chunks = Vec::with_capacity(items.len() / CHUNK + 1);
        let mut rest = items.into_iter().peekable();
        while rest.peek().is_some() {
            chunks.push(Arc::new(rest.by_ref().take(CHUNK).collect::<Vec<T>>()));
        }
        chunks
    }
}

impl<T: Clone> SharedVec<T> {
    /// A list of these elements, in order.
    pub fn from_vec(items: Vec<T>) -> Self {
        let len = items.len();
        let chunks = SharedVec::from_chunks_of(items);
        SharedVec { top: if chunks.is_empty() { None } else { Some(Arc::new(chunks)) }, len }
    }

    /// The chunk pointers, to change: copied first if an older list shares them.
    fn chunks_mut(&mut self) -> &mut Vec<Arc<Vec<T>>> {
        Arc::make_mut(self.top.get_or_insert_with(|| Arc::new(Vec::new())))
    }

    /// The element at `at`, to change. Its chunk is copied first if an older list shares it, so the older list is
    /// unchanged. `None` past the end.
    pub fn get_mut(&mut self, at: usize) -> Option<&mut T> {
        // Past the end is nothing, and copies nothing.
        if at >= self.len {
            return None;
        }
        let chunk = self.chunks_mut().get_mut(at / CHUNK)?;
        Arc::make_mut(chunk).get_mut(at % CHUNK)
    }

    /// Adds an element at the end.
    pub fn push(&mut self, item: T) {
        let chunks = self.chunks_mut();
        match chunks.last_mut() {
            Some(last) if last.len() < CHUNK => Arc::make_mut(last).push(item),
            _ => chunks.push(Arc::new(vec![item])),
        }
        self.len = self.len.saturating_add(1);
    }

    /// Replaces the element at `at`, returning the old one.
    pub fn set(&mut self, at: usize, item: T) -> Result<T, OutOfRange> {
        let slot = self.get_mut(at).ok_or(OutOfRange)?;
        Ok(std::mem::replace(slot, item))
    }

    /// Puts an element at `at` (`at == len()` is a push), moving the ones from there on up one place.
    pub fn insert(&mut self, at: usize, item: T) -> Result<(), OutOfRange> {
        if at > self.len {
            return Err(OutOfRange);
        }
        self.rebuild_from(at, |tail, offset| tail.insert(offset, item));
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// Takes out the element at `at`, moving the ones after it down one place. `None` past the end.
    pub fn remove(&mut self, at: usize) -> Option<T> {
        if at >= self.len {
            return None;
        }
        let mut removed = None;
        self.rebuild_from(at, |tail, offset| removed = Some(tail.remove(offset)));
        self.len = self.len.saturating_sub(1);
        removed
    }

    /// Rebuilds the chunks from the one holding `at` to the end, after `edit` has changed those elements (`offset` is
    /// `at` within them). The chunks before it stay shared.
    fn rebuild_from(&mut self, at: usize, edit: impl FnOnce(&mut Vec<T>, usize)) {
        let first = (at / CHUNK).min(self.chunks().len());
        let mut tail: Vec<T> = self.chunks().iter().skip(first).flat_map(|chunk| chunk.iter().cloned()).collect();
        edit(&mut tail, at.saturating_sub(first * CHUNK));
        let rebuilt = SharedVec::from_chunks_of(tail);
        let chunks = self.chunks_mut();
        chunks.truncate(first);
        chunks.extend(rebuilt);
    }
}

impl<T: Clone> FromIterator<T> for SharedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        SharedVec::from_vec(iter.into_iter().collect())
    }
}

impl<'a, T> IntoIterator for &'a SharedVec<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

/// The elements of a [`SharedVec`], front to back or back to front.
#[derive(Debug, Clone)]
pub struct Iter<'a, T> {
    list: &'a SharedVec<T>,
    front: usize,
    back: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        if self.front >= self.back {
            return None;
        }
        let item = self.list.get(self.front);
        self.front += 1;
        item
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.back.saturating_sub(self.front);
        (left, Some(left))
    }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front >= self.back {
            return None;
        }
        self.back -= 1;
        self.list.get(self.back)
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
